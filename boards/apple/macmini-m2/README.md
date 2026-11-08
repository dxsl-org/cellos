# Apple Mac mini M2 (J473, t8112)

Board descriptor for the Apple Mac mini (M2, 2023) running under m1n1 (stage 2 / hypervisor) per ADR-0032.

- **SoC:** Apple T8112 (M2, 8 cores: 4 Avalanche P-cores + 4 Blizzard E-cores)
- **Interrupt controller:** Apple AICv2 (FIQ for timer and fast IPIs)
- **UART console:** Samsung-style S5L UART (`apple,s5l-uart`)
- **DMA isolation:** Required (Apple DART t8110 IOMMU)
