"""Version 2 lifetime: bounded v1 tombstones, reclaimed only at a drained epoch barrier."""
from p4hfadapter.models.qwen3_5_0_8b.state import StageSessions


class EpochSessions:
    def __init__(self, stage, plan):
        self.stage, self.plan = stage, plan
        self.epoch = 1
        self.sessions = StageSessions(stage, plan)

    def advance(self, epoch):
        if type(epoch) is not int or epoch != self.epoch + 1 or self.sessions.active:
            raise ValueError("epoch requires next generation and zero active state")
        self.sessions = StageSessions(self.stage, self.plan)
        self.epoch = epoch

    def validate_epoch(self, epoch):
        if type(epoch) is not int or epoch != self.epoch:
            raise ValueError("stale session epoch")

