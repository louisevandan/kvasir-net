import unittest
import torch

from p4hfadapter.models.qwen3_5_0_8b.configuration import parse_plan
from p4hfadapter.models.qwen3_5_0_8b.state import StageSessions
from tests.fixtures.qwen_plans import plan
from tests.fixtures.qwen_rejection_stage import RejectionStage


class StateRefusalTests(unittest.TestCase):
    def test_total_request_budget_also_bounds_retired_identities(self):
        value = plan()
        value["limits"]["max_requests"] = 1
        stage = RejectionStage()
        sessions = StageSessions(stage, parse_plan(value))
        sessions.step("one", 0, 0, torch.tensor([[1]]))
        sessions.release("one")
        with self.assertRaises(ValueError):
            sessions.step("two", 0, 0, torch.tensor([[1]]))
        self.assertEqual(stage.calls, 1)
        self.assertEqual(len(sessions.retired), 1)

    def test_invalid_inputs_have_no_state_or_execution_effect(self):
        cases = [(1, 0, torch.tensor([[1]])), (0, 1, torch.tensor([[1]])),
                 (0, 0, torch.tensor([[-1]])), (0, 0, torch.tensor([[248056]])),
                 (0, 0, torch.tensor([[1]], dtype=torch.int32)),
                 (0, 0, torch.ones(2, 1, dtype=torch.int64)),
                 (0, 0, torch.ones(1, 2049, dtype=torch.int64))]
        for issue, position, tensor in cases:
            stage = RejectionStage()
            sessions = StageSessions(stage, parse_plan(plan()))
            with self.subTest(issue=issue, shape=tensor.shape), self.assertRaises(ValueError):
                sessions.step("session", issue, position, tensor)
            self.assertEqual(stage.calls, 0)
            self.assertEqual(sessions.active, {})

    def test_duplicate_and_released_session_do_not_execute_again(self):
        stage = RejectionStage()
        sessions = StageSessions(stage, parse_plan(plan()))
        tokens = torch.tensor([[1]])
        sessions.step("session", 0, 0, tokens)
        with self.assertRaises(ValueError):
            sessions.step("session", 0, 0, tokens)
        self.assertEqual(stage.calls, 1)
        self.assertEqual(sessions.release("session")["active_sessions"], 0)
        with self.assertRaises(ValueError):
            sessions.step("session", 0, 0, tokens)
        self.assertEqual(stage.calls, 1)

    def test_bad_hidden_dtype_shape_and_nan_have_zero_effect(self):
        for tensor in (torch.zeros(1, 2, 1023, dtype=torch.bfloat16), torch.zeros(1, 2, 1024),
                       torch.full((1, 2, 1024), float("nan"), dtype=torch.bfloat16)):
            stage = RejectionStage(start=3)
            sessions = StageSessions(stage, parse_plan(plan()))
            with self.assertRaises(ValueError):
                sessions.step("session", 0, 0, tensor)
            self.assertEqual(stage.calls, 0)
            self.assertEqual(sessions.active, {})
