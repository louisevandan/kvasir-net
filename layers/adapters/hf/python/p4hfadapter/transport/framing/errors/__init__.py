"""Framing refusals and terminal stream failures."""


class FramingError(Exception):
    """Base error for the local binary framing contract."""


class InvalidFrame(FramingError):
    """Header or outgoing payload is not in the supported wire format."""


class PayloadTooLarge(InvalidFrame):
    """Payload exceeds the explicit local frame limit."""


class TruncatedFrame(FramingError):
    """EOF arrived after part of a header or payload had been consumed."""


class TransportIOError(FramingError):
    """I/O did not complete; remote receipt and execution are unknown."""


class StreamClosed(FramingError):
    """This direction is terminal and must not be reused."""
