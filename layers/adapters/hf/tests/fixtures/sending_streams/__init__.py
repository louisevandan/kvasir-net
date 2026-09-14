"""Blocking-output doubles for framing failure injection."""

import io


class FragmentedOutput(io.BytesIO):
    flushed = False

    def write(self, data):
        return super().write(data[:2])

    def flush(self):
        self.flushed = True


class FailedPayloadOutput(io.BytesIO):
    def __init__(self, error):
        super().__init__()
        self.error = error

    def write(self, data):
        if self.tell() >= 16:
            raise self.error
        return super().write(data)


class InvalidCountOutput:
    def __init__(self, count):
        self.count = count

    def write(self, data):
        return self.count


class FailedFlushOutput(io.BytesIO):
    def flush(self):
        raise OSError("flush failed")
