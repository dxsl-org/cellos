Integer Q8_0 activation path — raw evidence
===========================================

Numbers in ../phase-01-integer-path.md come from these files.

Host benchmarks (cargo bench -p ai-engine --target x86_64-unknown-linux-gnu --bench cpu_engine)
----------------------------------------------------------------------------------------------
bench-int8-stories15M.txt   integer activations, stories15M-q8_0: decode 2.53 ms/token (395 tok/s),
                            matvec_q8_0 rows 6.76-6.79 GFLOP/s, int8 rows 12.06-12.75 GFLOP/s,
                            matvec_f32 11.66 GFLOP/s
bench-int8-smollm135m.txt   integer activations, SmolLM-135M-Q8_0: decode 23.56 ms/token (42.5 tok/s),
                            prefill 18.58 ms/prompt token, matvec_q8_0 rows 6.64-6.79 GFLOP/s,
                            int8 rows 12.03-12.68 GFLOP/s, matvec_f32 11.75 GFLOP/s

f32-staging A/B rows live in ../260914-cpu-engine-optimization/evidence/bench-final-*.txt
(stories15M decode 4.67 ms/token, SmolLM-135M decode 41.6 ms/token, kernel rows 6.7-6.8 GFLOP/s) and
the pre-profile baseline in bench-baseline-*.txt (-Oz: 24.1 / 211.5 ms/token, 1.34 GFLOP/s).

In-cell QEMU RV64 (CELLOS_AI_REAL_MODEL=stories15M-q8_0, scripts/run-ai-inference-oracle-qemu.sh)
-------------------------------------------------------------------------------------------------
oracle-int8-1.txt   24 tokens in 1595 ms, [ai-test] PASS, [ai-oracle] PASS (fixture + real-model
                    continuation scenarios)
oracle-int8-2.txt   24 tokens in 1601 ms, [ai-test] PASS, [ai-oracle] PASS

Before, on the same host and image flow: ../260914-cpu-engine-optimization/evidence/oracle-o2-{1,2}.txt
(6709 / 6986 ms) and oracle-oz-{baseline,2,3}.txt (7888 / 7916 / 7852 ms).

Cell image size (cargo build --release -p service-ai, riscv64gc-unknown-none-elf)
---------------------------------------------------------------------------------
-Oz 236,328 B | -O2 f32 staging 230,288 B | -O2 integer (shipped) 225,048 B

Numerics evidence
-----------------
- Phase-01 "honest cost" section: derived-bound test, bit-exact case, and the measured margin shift.
- models/tiny-llama-64.golden.txt before/after: greedy ids identical (155,215,256,177,163,39,126,211),
  weakest margin 0.439722 -> 0.380074, logits and embed values shifted as regenerated.
- scripts/gen-ai-test-model.py --check: OK (model bytes unchanged, golden matches the generator).
- The pre-Rust probe of the same question (f32 vs integer reference, ids and margins) was run with a
  throwaway script; its result — same ids, margins 0.44 -> 0.38 — is what the phase doc cites, and the
  committed generator reproduces it in `--check` form.

Unit suites (x86_64 host)
-------------------------
tensor-math 29 passed (incl. 65 536-pattern f16 round-trip, quantizer bounds/NaN/shape errors,
integer-kernel exactness and bound) | ai-engine 12 passed (incl. golden ids and embedding vs the
independent reference) | ai-proto 6 | ai-sdk 8.
