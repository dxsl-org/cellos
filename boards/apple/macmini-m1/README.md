# Apple Mac mini M1 (J274, t8103)

Board descriptor for the Apple Mac mini (M1, 2020) running under m1n1 (stage 2 / hypervisor) per ADR-0032.

- **SoC:** Apple T8103 (M1, 8 cores: 4 Firestorm P-cores + 4 Icestorm E-cores)
- **Interrupt controller:** Apple AIC1 (FIQ for timer and fast IPIs)
- **UART console:** Samsung-style S5L UART (`apple,s5l-uart`)
- **DMA isolation:** Required (Apple DART IOMMU)
