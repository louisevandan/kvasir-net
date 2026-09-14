"""Version 1 framing layout; payload semantics belong to the model adapter."""

from struct import Struct

HEADER = Struct("!4sB3sQ")
MAGIC = b"P4HF"
VERSION = 1
RESERVED = b"\x00\x00\x00"
