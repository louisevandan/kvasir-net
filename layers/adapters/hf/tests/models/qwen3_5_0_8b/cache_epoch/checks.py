import unittest
from types import SimpleNamespace
import torch
from safetensors.torch import save,load
from p4hfadapter.models.qwen3_5_0_8b.cache_export import compare,export
from p4hfadapter.models.qwen3_5_0_8b.epoch import EpochSessions
from p4hfadapter.models.qwen3_5_0_8b.configuration import parse_plan
from tests.fixtures.qwen_plans import plan
from tests.fixtures.qwen_rejection_stage import RejectionStage

class CacheEpochTests(unittest.TestCase):
    def cache(self):
        return SimpleNamespace(layers=[SimpleNamespace(keys=torch.zeros(2,3),recurrent_states={'state':torch.ones(3,4)}) for _ in range(2)])
    def test_every_cache_element_and_global_layer_is_compared(self):
        reference=self.cache();local=SimpleNamespace(layers=reference.layers[1:])
        body=export(local,1);result=compare(body,reference,1,2)
        self.assertEqual(result['elements'],18)
        value=load(body);value['1.keys'][0,0]=10
        with self.assertRaisesRegex(AssertionError,'cache content mismatch'):compare(save(value),reference,1,2)
    def test_cache_members_shape_and_dtype_are_not_position_only(self):
        reference=self.cache();body=export(reference,0)
        for kind in ('member','shape','dtype'):
            value=load(body)
            if kind=='member':del value['1.keys']
            if kind=='shape':value['1.keys']=value['1.keys'].reshape(3,2)
            if kind=='dtype':value['1.keys']=value['1.keys'].double()
            with self.assertRaises(AssertionError):compare(save(value),reference,0,2)
    def test_live_epoch_refusal_preserves_state_and_effect_count(self):
        stage=RejectionStage();epochs=EpochSessions(stage,parse_plan(plan()))
        epochs.sessions.step('same',0,0,torch.tensor([[1]]));before=epochs.sessions
        with self.assertRaises(ValueError):epochs.advance(2)
        self.assertIs(epochs.sessions,before);self.assertEqual(stage.calls,1);self.assertEqual(epochs.epoch,1)
    def test_three_drained_epochs_reuse_same_id_with_bounded_tombstones(self):
        raw=plan();raw['limits']['max_requests']=1
        stage=RejectionStage();epochs=EpochSessions(stage,parse_plan(raw))
        for epoch in (1,2,3):
            epochs.validate_epoch(epoch)
            epochs.sessions.step('same',0,0,torch.tensor([[1]]));epochs.sessions.release('same')
            self.assertEqual(len(epochs.sessions.retired),1)
            if epoch<3:
                epochs.advance(epoch+1)
                with self.assertRaises(ValueError):epochs.validate_epoch(epoch)
        self.assertEqual(stage.calls,3)
