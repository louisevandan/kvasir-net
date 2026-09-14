"""Blocking-input doubles for framing failure injection."""

import io


class FragmentedInput(io.BytesIO):
    def read(self, size=-1):
        return super().read(min(2, size))


class HeaderOnlyInput:
    def __init__(self, header):
        self.header = header
        self.calls = 0

    def read(self, size):
        self.calls += 1
        if self.calls > 1:
            raise AssertionError("body was read")
        return self.header


class FailedInput:
    def __init__(self, error):
        self.error = error

    def read(self, size):
        raise self.error


class NotReadyInput:
    def read(self, size):
        return None
