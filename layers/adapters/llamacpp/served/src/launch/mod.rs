//! Turning a placement into the command line that realises it.
//!
//! This is the knowledge a concrete adapter exists to hold. The plan states
//! intent — which device, how much of it, how many sequences, how long a
//! context — and what those become is a fact about one server. llama.cpp calls
//! them `-dev`, `-ts`, `-c`, `--parallel`; vLLM calls them
//! `--tensor-parallel-size` and `--max-model-len`; SGLang calls them something
//! else again. A plan carrying `-ts` would be a plan that had learned
//! llama.cpp, and then no other backend could read it.
//!
//! Composing the arguments is separated from running them because the first is
//! a pure function of a plan and the second is a process. One can be checked
//! exhaustively without starting anything, and it is the half that is easy to
//! get quietly wrong: a batch too small to hold one prompt makes a server admit
//! prefills one at a time however fast work is offered to it, and nothing about
//! that looks like a mistake from outside.

use crate::flavour::Flavour;
use crate::plan::{Plan, Role, Start};

pub mod process;

/// The arguments that start this plan's backend.
///
/// An error when the flavour has no way to be started by us — every backend can
/// be attached to, and being able to start one is the extra.
pub fn arguments(flavour: Flavour, plan: &Plan, start: &Start) -> Result<Vec<String>, String> {
    match flavour {
        Flavour::LlamaCpp => Ok(llama_cpp(plan, start)),
        // Neither runs on the machines this was written against, so composing a
        // command line for them would be a guess presented as knowledge. They
        // attach to a server that is already there, which is what every plan
        // without a `start` does.
        Flavour::Vllm | Flavour::Sglang => Err(format!(
            "{} plans cannot start a server here: give the plan an endpoint to \
             attach to instead",
            flavour.name()
        )),
    }
}

/// llama.cpp's own names for what the plan asked for.
fn llama_cpp(plan: &Plan, start: &Start) -> Vec<String> {
    let mut args = vec![
        "-m".into(),
        start.weights.clone(),
        "--host".into(),
        plan.endpoint.host.clone(),
        "--port".into(),
        plan.endpoint.port.to_string(),
        "-c".into(),
        start.context.to_string(),
        "--parallel".into(),
        start.slots.to_string(),
        "-b".into(),
        start.batch.to_string(),
        "-ub".into(),
        start.ubatch.to_string(),
        "-a".into(),
        plan.model.clone(),
    ];

    // A worker holds a share and serves nothing, so it is the other binary
    // entirely and takes only the device it is pinned to.
    if plan.role == Role::Worker {
        let mut worker = vec![
            "-H".into(),
            plan.endpoint.host.clone(),
            "-p".into(),
            plan.endpoint.port.to_string(),
        ];
        if let Some(device) = &plan.device {
            worker.push("-d".into());
            worker.push(device.clone());
        }
        return worker;
    }

    // The shares held elsewhere, in the order the plan listed them, and the
    // devices to use named explicitly. Without naming them a front takes every
    // local card as well as the remote ones, which on a machine holding a
    // worker means using the same card twice.
    if !plan.workers.is_empty() {
        args.push("--rpc".into());
        args.push(
            plan.workers
                .iter()
                .map(|worker| format!("{}:{}", worker.host, worker.port))
                .collect::<Vec<_>>()
                .join(","),
        );
        let mut devices = vec![plan.device.clone().unwrap_or_else(|| "CUDA0".into())];
        devices.extend((0..plan.workers.len()).map(|index| format!("RPC{index}")));
        args.push("-dev".into());
        args.push(devices.join(","));
    } else if let Some(device) = &plan.device {
        args.push("-dev".into());
        args.push(device.clone());
    }

    // Every layer on a device, always. Left to itself the server puts what does
    // not fit on the CPU, and a remote share cannot reach a tensor in host
    // memory: the worker refuses the graph with an invalid pointer rather than
    // running slowly.
    args.push("-ngl".into());
    args.push("999".into());
    args
}

#[cfg(test)]
mod tests;
