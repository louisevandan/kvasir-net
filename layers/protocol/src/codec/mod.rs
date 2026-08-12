//! Bounded P4 binary framing and payload codec.

mod fields;
mod frame;
mod kind;
mod payload;

pub use frame::{
    RoutedMessage, decode_message, decode_routed_message, encode_message, encode_routed_message,
    read_message, read_routed_message, write_message, write_routed_message,
};

#[cfg(test)]
mod tests;
