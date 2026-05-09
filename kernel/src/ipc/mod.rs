//! Beast OS IPC — Scheduler-Integrated SPSC Channels
//!
//! Wraps `beast_ring::SpscRing` with kernel scheduler integration:
//! - Blocking dequeue with automatic task suspension
//! - Wake-on-push with SPSC-aware priority boosting
//! - Zero overhead when ring is non-empty (fast path = plain SPSC)
//!
//! This is **the core Beast OS innovation**: the scheduler knows
//! *exactly why* a task blocked and makes perfect boosting decisions.

pub mod channel;
