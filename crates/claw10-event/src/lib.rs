//! Event bus abstraction untuk Claw10.
//!
//! Menyediakan trait `EventBus` yang dapat diimplementasikan oleh:
//! - `InMemoryEventBus` untuk testing
//! - `NatsEventBus` untuk produksi (feature flag `nats`)
//!
//! Event types mencerminkan lifecycle agen, task, dan memory.

pub mod bus;
pub mod events;
pub mod inmemory;

#[cfg(feature = "nats")]
pub mod nats;

pub use bus::{EventBus, EventBusError, EventHandler, SubscriptionId};
pub use events::Claw10Event;
pub use inmemory::InMemoryEventBus;

#[cfg(feature = "nats")]
pub use nats::NatsEventBus;
