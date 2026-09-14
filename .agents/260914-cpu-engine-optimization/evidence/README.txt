CPU inference engine throughput — raw evidence
=============================================

Every number in ../phase-01-kernel-throughput.md comes from one of these files.

Host benchmarks (cargo bench -p ai-engine --target x86_64-unknown-linux-gnu --bench cpu_engine)
----------------------------------------------------------------------------------------------
bench-baseline-stories15M.txt   -Oz, stories15M-q8_0,     decode 24.1 ms/token
bench-baseline-smollm135m.txt   -Oz, SmolLM-135M-Q8_0,    decode 211.5 ms/token
bench-o3-stories15M.txt         -O3, stories15M-q8_0,     decode 3.1 ms/token
bench-o3-smollm135m.txt         -O3, SmolLM-135M-Q8_0,    decode 28.6 ms/token
bench-fused-*.txt               fused decode (reverted): 7.5 GFLOP/s at -O3, worse than staged
bench-final-stories15M.txt      -O2, stories15M-q8_0,     decode 4.7 ms/token   <- shipped config
bench-final-smollm135m.txt      -O2, SmolLM-135M-Q8_0,    decode 41.6 ms/token  <- shipped config

In-cell QEMU RV64 (CELLOS_AI_REAL_MODEL=stories15M-q8_0, scripts/run-ai-inference-oracle-qemu.sh)
-------------------------------------------------------------------------------------------------
oracle-oz-baseline.txt   -Oz  24 tokens in 7888 ms
oracle-oz-2.txt          -Oz  24 tokens in 7916 ms
oracle-oz-3.txt          -Oz  24 tokens in 7852 ms
oracle-o2-1.txt          -O2  24 tokens in 6986 ms   <- shipped config
oracle-o2-2.txt          -O2  24 tokens in 6709 ms   <- shipped config
oracle-o3.txt            -O3  24 tokens in 8723 ms
oracle-o3-2.txt          -O3  24 tokens in 8756 ms
oracle-o3-3.txt          -O3  24 tokens in 8843 ms

Each oracle log also carries the cell-observed timing line added in this slice:
  USER: Cellos > [ai-test] generate: 24 tokens in <ms> ms (N tokens/s x1000, 13 polls)

Cell image size (cargo build --release -p service-ai, riscv64gc-unknown-none-elf)
---------------------------------------------------------------------------------
-Oz 236,328 B | -O2 230,288 B (shipped) | -O3 228,392 B

Consumer gates on the rebuilt canonical image (disk_v3.img from gen_disk.ps1)
-----------------------------------------------------------------------------
http-infer       1 passed  47.18 s
hypha-local-ai   1 passed   8.49 s
hypha-boot       1 passed   6.59 s
hypha-p3-boot    1 passed   6.53 s
Host suites: tensor-math 22, ai-engine 12, ai-proto 6, ai-sdk 8 — all passed.

Codegen checks (cargo rustc --emit=asm, function body read directly)
--------------------------------------------------------------------
-Oz x86_64 matvec_q8_0:  6 out-of-line calls per block (q8_0_block_to_f32, accumulate_products,
                         chunks_exact, Zip::new, q8_0_row_bytes, one indirect)
-O3 x86_64 matvec_q8_0:  inlined, block loop unrolled into scalar f32 (32 mulss + 35 addss), no
                         vector instructions — the win is inlining plus the unroll, not SIMD
-O3 aarch64:             scalar too (0 NEON instructions in the kernel)
