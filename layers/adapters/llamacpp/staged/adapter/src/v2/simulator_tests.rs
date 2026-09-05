//! What the scheduler must do, checked without a GPU.
//!
//! Each of these would have caught a mistake this pipeline actually made, and
//! none of them needs a machine with two cards or twenty minutes of wall clock.

use super::scheduler::Phase;
use super::simulator::{PipelineShape, Simulation};

/// One tick of a golden trace: when it was issued, and what it carried.
type TracedTick = (usize, Vec<(String, Phase, usize)>);

fn assert_clean(simulation: &Simulation, what: &str) {
    assert!(
        simulation.violations.is_empty(),
        "{what}: {:#?}",
        simulation.violations,
    );
}

#[test]
fn a_mixed_load_holds_every_invariant() {
    let mut simulation = Simulation::new(PipelineShape::default());
    // The shape the harness runs: a few long prompts among many short ones.
    for index in 0..8 {
        let prompt = match index % 4 {
            0 => 19,
            1 => 202,
            2 => 564,
            _ => 1290,
        };
        simulation.admit(&format!("request-{index}"), prompt, 4);
    }
    let ticks = simulation.run(4000);
    assert_clean(&simulation, "mixed load");
    assert!(ticks < 4000, "every request should finish; used {ticks} ticks");
}

#[test]
fn a_recurrent_model_holds_them_too() {
    // Equal per-sequence widths, where one ready decode used to cut every
    // prompt to a single row.
    let mut simulation = Simulation::new(PipelineShape {
        equal_sequence_ubatch: true,
        ..PipelineShape::default()
    });
    for index in 0..6 {
        simulation.admit(&format!("request-{index}"), 500, 3);
    }
    let ticks = simulation.run(6000);
    assert_clean(&simulation, "recurrent load");
    assert!(ticks < 6000, "every request should finish; used {ticks} ticks");
}

#[test]
fn a_prompt_is_never_starved_by_a_stream_of_decodes() {
    // One long prompt against seven sequences that decode for a long time. If
    // the cohort split let decodes monopolise the pipeline, the prompt would
    // never tile and the run would exhaust its budget.
    let mut simulation = Simulation::new(PipelineShape {
        equal_sequence_ubatch: true,
        ..PipelineShape::default()
    });
    simulation.admit("prompt", 2000, 1);
    for index in 0..7 {
        simulation.admit(&format!("decoder-{index}"), 8, 200);
    }
    let ticks = simulation.run(20_000);
    assert_clean(&simulation, "starvation load");
    assert!(
        ticks < 20_000,
        "a long prompt must finish beside continuous decoding; used {ticks} ticks",
    );
}

#[test]
fn raising_the_fragment_limit_changes_nothing_a_test_can_see_but_the_pacing() {
    // Both limits must hold every invariant. This is the guard the shipped
    // default rests on: `prefill_fragments > 1` is experimental, and the thing
    // that makes it experimental is that it has no fragment ledger behind it -
    // not that it breaks these.
    for fragments in [1, 2, 4] {
        let mut simulation = Simulation::new(PipelineShape {
            prefill_fragments: fragments,
            ..PipelineShape::default()
        });
        simulation.admit("long", 1290, 2);
        simulation.admit("short", 19, 2);
        let ticks = simulation.run(4000);
        assert_clean(&simulation, &format!("fragments={fragments}"));
        assert!(ticks < 4000, "fragments={fragments} did not finish");
    }
}

#[test]
fn one_fragment_makes_a_prompt_wait_a_whole_lap_and_more_do_not() {
    // The claim the fragment work rests on, stated as a count rather than as a
    // throughput number that a busy pipeline can hide.
    //
    // The batch capacity has to be the thing that splits the prompt, or there
    // is nothing to overlap: the default 512 swallows a 260-row prompt whole
    // and both limits issue it on tick zero, which is how this test first
    // failed. At 64 rows a batch it is five fragments - one lap apart when
    // only one may travel, back to back when four may.
    let issue_ticks = |fragments: u32| {
        let mut simulation = Simulation::new(PipelineShape {
            prefill_fragments: fragments,
            batch_capacity: 64,
            physical_capacity: 64,
            ..PipelineShape::default()
        });
        simulation.admit("only", 260, 1);
        simulation.run(4000);
        assert!(simulation.violations.is_empty(), "{:#?}", simulation.violations);
        simulation
            .trace
            .iter()
            .filter(|batch| batch.rows.iter().any(|(_, phase, _)| *phase == Phase::Prefill))
            .map(|batch| batch.tick)
            .collect::<Vec<_>>()
    };

    let serial = issue_ticks(1);
    let overlapped = issue_ticks(4);
    assert_eq!(serial.len(), overlapped.len(), "the same rows are issued either way");
    let serial_span = serial.last().unwrap() - serial.first().unwrap();
    let overlapped_span = overlapped.last().unwrap() - overlapped.first().unwrap();
    assert!(
        overlapped_span < serial_span,
        "more fragments should compress the prompt's issue window: {serial:?} against {overlapped:?}",
    );
}

#[test]
fn the_shipped_default_reproduces_its_recorded_trace() {
    // A golden trace of limit=1, so a scheduler that is refactored has to make
    // the same decisions rather than merely pass the invariants. Two prompts of
    // very different lengths and a capacity that forces sharing is enough to
    // pin the water-fill, the round robin and the lap pacing at once.
    let mut simulation = Simulation::new(PipelineShape {
        stages: 2,
        physical_capacity: 8,
        batch_capacity: 64,
        prefill_fragments: 1,
        equal_sequence_ubatch: false,
    });
    simulation.admit("long", 20, 1);
    simulation.admit("short", 4, 1);
    simulation.run(200);
    assert!(simulation.violations.is_empty(), "{:#?}", simulation.violations);

    let trace: Vec<TracedTick> = simulation
        .trace
        .iter()
        .map(|batch| (batch.tick, batch.rows.clone()))
        .collect();
    let rendered = format!("{trace:?}");
    // Recorded from the shipped scheduler. A change here is a change in
    // scheduling, and has to be explained rather than accepted.
    assert_eq!(
        rendered,
        GOLDEN_LIMIT_ONE,
        "the scheduler's decisions changed",
    );
}

const GOLDEN_LIMIT_ONE: &str = "[(0, [(\"long\", Prefill, 20), (\"short\", Prefill, 4)]), (2, [(\"short\", Decode, 1), (\"long\", Decode, 1)])]";

#[test]
fn a_ready_decode_does_not_cut_a_prompt_to_a_single_row() {
    // The defect the cohort split exists to prevent, stated as a width rather
    // than as a throughput number.
    //
    // On a model that forces equal per-sequence widths, a decode has exactly
    // one row to give, so a batch that admits both a decode and a prompt is a
    // batch one row wide - and the prompt with two thousand rows ready sends
    // one of them. The measured symptom on the 35B was 944.6 rows ready
    // against 9.85 issued.
    //
    // Removing the split in `plan_equal_ordinary` passed every other test in
    // this file, which is why this one is here: finishing is not the property
    // that was lost, width is.
    let mut simulation = Simulation::new(PipelineShape {
        equal_sequence_ubatch: true,
        ..PipelineShape::default()
    });
    simulation.admit("prompt", 2000, 1);
    for index in 0..7 {
        simulation.admit(&format!("decoder-{index}"), 8, 200);
    }
    simulation.run(20_000);
    assert_clean(&simulation, "mixed cohort load");

    let prefill: Vec<usize> = simulation
        .trace
        .iter()
        .flat_map(|batch| batch.rows.iter())
        .filter(|(id, phase, _)| id == "prompt" && *phase == Phase::Prefill)
        .map(|(_, _, rows)| *rows)
        .collect();
    let total: usize = prefill.iter().sum();
    assert_eq!(total, 2000, "the whole prompt is issued either way");
    let mean = total as f64 / prefill.len() as f64;
    assert!(
        mean >= 16.0,
        "a prompt sharing the pipeline with decodes was issued {} rows a batch \
         over {} batches; a decode is cutting it to its own width",
        mean,
        prefill.len(),
    );
}
