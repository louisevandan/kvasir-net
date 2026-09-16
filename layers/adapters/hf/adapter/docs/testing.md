# testing

tests/retained.rs consumes a real OS Python pipe. Model conformance is run by HF scripts/verification/event_qwen/run.py over P4 TCP.

The [integration specification](../../docs/integration/README.md) owns the wire format, budgets, deployment and error semantics.

In the default parallel workspace tests, only the Python startup section of fixture LOAD is isolated with a process-local mutex.
The 300ms fixture timeout stays. After readiness, execution, queue saturation and cancellation/cleanup of different tests run in parallel.
A mutation that removes startup_isolation detects the existing LOAD timeout counterexample under the default test thread configuration.
