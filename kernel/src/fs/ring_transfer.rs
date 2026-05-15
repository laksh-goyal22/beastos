//! Zero-Copy File Transfer via SPSC Rings
//!
//! Uses BeastSPSCRing for 20GB/s token passing with scheduler boosting.

use crate::fs::token::{Token, TokenId, Permissions};
use crate::ipc::spsc_with_boost::BeastSPSCRing;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use crate::kprintln;

/// Transfer request message (must be Copy for SPSC ring)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct FileTransferRequest {
    pub request_id: u64,
    pub source_token_high: u64,
    pub source_token_low: u64,
    pub target_process: u64,
    pub transfer_type: u8,  // 0 = TokenOnly, 1 = TokenWithContent
    pub permissions: u8,
    pub expires_in_seconds: u64,
}

impl FileTransferRequest {
    pub fn source_token_id(&self) -> TokenId {
        TokenId::from_raw(self.source_token_high, self.source_token_low)
    }
    
    pub fn transfer_type(&self) -> TransferType {
        match self.transfer_type {
            0 => TransferType::TokenOnly,
            _ => TransferType::TokenWithContent,
        }
    }
    
    pub fn permissions(&self) -> Permissions {
        Permissions(self.permissions)
    }
}

/// Transfer response message
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct FileTransferResponse {
    pub request_id: u64,
    pub success: u8,
    pub new_token_high: u64,
    pub new_token_low: u64,
    pub content_hash: [u8; 32],
    pub error_code: u8,
}

impl FileTransferResponse {
    pub fn success(&self) -> bool {
        self.success != 0
    }
    
    pub fn new_token_id(&self) -> TokenId {
        TokenId::from_raw(self.new_token_high, self.new_token_low)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TransferType {
    TokenOnly = 0,
    TokenWithContent = 1,
}

/// SPSC Ring based file transfer manager using BeastSPSCRing
pub struct RingTransferManager {
    /// Incoming transfer requests ring
    request_ring: Arc<BeastSPSCRing<FileTransferRequest, 256>>,
    /// Outgoing transfer responses ring
    response_ring: Arc<BeastSPSCRing<FileTransferResponse, 256>>,
    /// Active transfers
    active_transfers: Mutex<alloc::collections::BTreeMap<u64, TransferState>>,
    /// Next request ID
    next_request_id: AtomicU64,
}

/// State of an ongoing transfer
#[allow(dead_code)]
struct TransferState {
    source_token: Token,
    target_process: u64,
    created: u64,
}

impl RingTransferManager {
    pub fn new() -> Self {
        Self {
            request_ring: Arc::new(BeastSPSCRing::new()),
            response_ring: Arc::new(BeastSPSCRing::new()),
            active_transfers: Mutex::new(alloc::collections::BTreeMap::new()),
            next_request_id: AtomicU64::new(1),
        }
    }
    
    /// Set consumer task for request ring (for scheduler boosting)
    pub fn set_request_consumer(&self, task_id: crate::scheduler::task::TaskId) {
        self.request_ring.set_consumer(task_id);
    }
    
    /// Set consumer task for response ring
    pub fn set_response_consumer(&self, task_id: crate::scheduler::task::TaskId) {
        self.response_ring.set_consumer(task_id);
    }
    
    /// Get request ring for producer side
    pub fn request_ring(&self) -> Arc<BeastSPSCRing<FileTransferRequest, 256>> {
        self.request_ring.clone()
    }
    
    /// Send a file to another process (zero-copy via token)
    pub fn send_file(
        &self,
        source_token_id: TokenId,
        target_process: u64,
        transfer_type: TransferType,
        permissions: Permissions,
        expires_in_seconds: Option<u64>,
        sender_user_id: u64,
    ) -> Result<u64, TransferError> {
        // 1. Look up source token
        let source_token = Token::lookup(source_token_id)
            .ok_or(TransferError::TokenNotFound)?;
        
        // 2. Verify sender has permission to share
        if !source_token.check_permission(Permissions::SHARE, sender_user_id) {
            return Err(TransferError::PermissionDenied);
        }
        
        // 3. Create request
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let request = FileTransferRequest {
            request_id,
            source_token_high: source_token_id.high(),
            source_token_low: source_token_id.low(),
            target_process,
            transfer_type: transfer_type as u8,
            permissions: permissions.bits(),
            expires_in_seconds: expires_in_seconds.unwrap_or(0),
        };
        
        // 4. Store transfer state
        {
            let mut transfers = self.active_transfers.lock();
            transfers.insert(request_id, TransferState {
                source_token,
                target_process,
                created: Self::current_time(),
            });
        }
        
        // 5. Enqueue request (50ns, zero-copy, auto-boosts consumer)
        self.request_ring.enqueue(request)
            .map_err(|_| TransferError::RingFull)?;
        
        kprintln!("[XFER] Sent file request {} (token: {})", request_id, source_token_id.high());
        Ok(request_id)
    }
    
    /// Receive pending transfers (called by target process)
    pub fn receive_transfers(&self) -> Vec<FileTransferRequest> {
        let mut requests = Vec::new();
        while let Some(request) = self.request_ring.try_dequeue() {
            requests.push(request);
        }
        requests
    }
    
    /// Wait for a transfer request (blocks using SPSC wait)
    pub fn wait_for_transfer(&self) -> FileTransferRequest {
        self.request_ring.wait_dequeue()
    }
    
    /// Accept a transfer request (creates new token pointing to same content)
    pub fn accept_transfer(
        &self,
        request: &FileTransferRequest,
        recipient_user_id: u64,
        recipient_path: &str,
    ) -> Result<TokenId, TransferError> {
        // 1. Check if request expired
        if request.expires_in_seconds > 0 {
            if Self::current_time() > request.expires_in_seconds {
                return Err(TransferError::Expired);
            }
        }
        
        // 2. Look up source token
        let source_token = Token::lookup(request.source_token_id())
            .ok_or(TransferError::TokenNotFound)?;
        
        // 3. Create new token for recipient (points to SAME content!)
        let new_token = Token::new_file(
            recipient_path,
            source_token.content_hash,
            recipient_user_id,
            source_token.size,
            request.permissions(),
        );
        let new_token_id = new_token.id;
        new_token.register();
        
        // 4. Send response
        let response = FileTransferResponse {
            request_id: request.request_id,
            success: 1,
            new_token_high: new_token_id.high(),
            new_token_low: new_token_id.low(),
            content_hash: *source_token.content_hash.as_bytes(),
            error_code: 0,
        };
        
        self.response_ring.enqueue(response)
            .map_err(|_| TransferError::RingFull)?;
        
        kprintln!("[XFER] Accepted transfer, created token: {}", new_token_id.high());
        Ok(new_token_id)
    }
    
    /// Decline a transfer request
    pub fn decline_transfer(&self, request_id: u64, error_code: u8) -> Result<(), TransferError> {
        let response = FileTransferResponse {
            request_id,
            success: 0,
            new_token_high: 0,
            new_token_low: 0,
            content_hash: [0; 32],
            error_code,
        };
        
        self.response_ring.enqueue(response)
            .map_err(|_| TransferError::RingFull)
    }
    
    /// Wait for transfer response (blocking)
    pub fn wait_for_response(&self) -> FileTransferResponse {
        self.response_ring.wait_dequeue()
    }
    
    /// Check for responses (non-blocking)
    pub fn check_responses(&self) -> Vec<FileTransferResponse> {
        let mut responses = Vec::new();
        while let Some(response) = self.response_ring.try_dequeue() {
            responses.push(response);
        }
        responses
    }
    
    /// Transfer file content directly (for legacy/non-Beast targets)
    pub fn transfer_content_direct(&self, token_id: TokenId) -> Result<&[u8], TransferError> {
        let _token = Token::lookup(token_id).ok_or(TransferError::TokenNotFound)?;
        Err(TransferError::ContentNotFound)
    }
    
    /// Clean up completed transfers
    pub fn cleanup(&self, request_id: u64) {
        self.active_transfers.lock().remove(&request_id);
    }
    
    fn current_time() -> u64 {
        crate::drivers::pit::get_ticks() / 100
    }
}

/// Token-only ring for bulk transfers (128-bit tokens only)
pub struct TokenTransferRing {
    ring: BeastSPSCRing<u128, 1024>,
}

impl TokenTransferRing {
    pub fn new() -> Self {
        Self {
            ring: BeastSPSCRing::new(),
        }
    }
    
    /// Set consumer task for scheduler boosting
    pub fn set_consumer(&self, task_id: crate::scheduler::task::TaskId) {
        self.ring.set_consumer(task_id);
    }
    
    /// Send token (zero-copy, 50ns, auto-boosts consumer)
    pub fn send_token(&self, token_id: TokenId) -> Result<(), TransferError> {
        self.ring.enqueue(token_id.0)
            .map_err(|_| TransferError::RingFull)?;
        Ok(())
    }
    
    /// Receive token (non-blocking)
    pub fn receive_token(&self) -> Option<TokenId> {
        self.ring.try_dequeue().map(TokenId)
    }
    
    /// Wait for token (blocks with scheduler boosting)
    pub fn wait_for_token(&self) -> TokenId {
        TokenId(self.ring.wait_dequeue())
    }
    
    /// Batch send multiple tokens
    pub fn send_tokens(&self, tokens: &[TokenId]) -> usize {
        let mut sent = 0;
        for token in tokens {
            if self.send_token(*token).is_err() {
                break;
            }
            sent += 1;
        }
        sent
    }
    
    /// Receive all available tokens
    pub fn receive_all(&self) -> Vec<TokenId> {
        let mut tokens = Vec::new();
        while let Some(token) = self.receive_token() {
            tokens.push(token);
        }
        tokens
    }
}

/// Bulk directory transfer
pub struct BulkDirectoryTransfer {
    token_ring: TokenTransferRing,
    total_tokens: usize,
    transferred_tokens: AtomicU64,
}

impl BulkDirectoryTransfer {
    pub fn new() -> Self {
        Self {
            token_ring: TokenTransferRing::new(),
            total_tokens: 0,
            transferred_tokens: AtomicU64::new(0),
        }
    }
    
    pub fn token_ring(&self) -> &TokenTransferRing {
        &self.token_ring
    }
    
    /// Send an entire directory (all files recursively)
    pub fn send_directory(&mut self, directory_token_id: TokenId) -> Result<usize, TransferError> {
        let dir_token = Token::lookup(directory_token_id)
            .ok_or(TransferError::TokenNotFound)?;
        
        if dir_token.token_type != crate::fs::token::TokenType::Folder {
            return Err(TransferError::NotADirectory);
        }
        
        // Recursively collect all tokens
        let mut all_tokens = Vec::new();
        self.collect_subtree_tokens(directory_token_id, &mut all_tokens);
        
        self.total_tokens = all_tokens.len();
        
        // Send all tokens through the ring
        for token in all_tokens {
            self.token_ring.send_token(token)?;
            self.transferred_tokens.fetch_add(1, Ordering::Relaxed);
        }
        
        Ok(self.total_tokens)
    }
    
    /// Receive all tokens from a directory transfer
    pub fn receive_directory(&self) -> Vec<TokenId> {
        self.token_ring.receive_all()
    }
    
    /// Progress as percentage
    pub fn progress(&self) -> f64 {
        if self.total_tokens == 0 {
            100.0
        } else {
            (self.transferred_tokens.load(Ordering::Relaxed) as f64 / self.total_tokens as f64) * 100.0
        }
    }
    
    fn collect_subtree_tokens(&self, token_id: TokenId, tokens: &mut Vec<TokenId>) {
        tokens.push(token_id);
        
        if let Some(token) = Token::lookup(token_id) {
            for child_id in &token.children {
                self.collect_subtree_tokens(*child_id, tokens);
            }
        }
    }
}

#[derive(Debug)]
pub enum TransferError {
    TokenNotFound,
    PermissionDenied,
    RingFull,
    Expired,
    ContentNotFound,
    NotADirectory,
}

impl core::fmt::Display for TransferError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TransferError::TokenNotFound => write!(f, "Token not found"),
            TransferError::PermissionDenied => write!(f, "Permission denied"),
            TransferError::RingFull => write!(f, "Transfer ring full"),
            TransferError::Expired => write!(f, "Transfer request expired"),
            TransferError::ContentNotFound => write!(f, "File content not found"),
            TransferError::NotADirectory => write!(f, "Not a directory"),
        }
    }
}