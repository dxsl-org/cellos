# Ockam Integration Strategy for ViOS

## 1. Architecture Overview

### Why Ockam?
Ockam provides **end-to-end encrypted channels** between devices, perfect for ViOS's robotics/IoT use case where:
- Robot A needs to securely communicate with Robot B
- Sensor data must be encrypted in transit
- Mutual authentication is critical

### Integration Approach: User-Space Service

```
┌─────────────────────────────────────────┐
│           ViOS Kernel                   │
│  ┌──────────┐      ┌──────────────┐    │
│  │ VFS/IPC  │◄────►│ Net Driver   │    │
│  └──────────┘      └──────────────┘    │
└─────────────────────────────────────────┘
           ▲                    ▲
           │ IPC                │ Raw Packets
           ▼                    ▼
┌──────────────────┐   ┌──────────────────┐
│  Ockam Service   │   │  Network Stack   │
│  (User Space)    │──►│  (smoltcp)       │
└──────────────────┘   └──────────────────┘
```

**Key Decision**: Ockam runs as a **User-Space Application**, NOT in the kernel.

## 2. Implementation Plan

### Phase 1: Ockam Core Integration
- [ ] Add `ockam` crate to a new `apps/vios-ockam-service`
- [ ] Create minimal Ockam node (identity, vault)
- [ ] Implement basic secure channel creation

### Phase 2: IPC Bridge
- [ ] Define syscalls for "send_encrypted" / "receive_encrypted"
- [ ] Create IPC channel: App → Ockam Service → Network Driver
- [ ] Implement message routing

### Phase 3: Testing
- [ ] Create demo: Robot A sends encrypted message to Robot B
- [ ] Verify E2EE using Ockam's cryptographic guarantees

## 3. Crate Structure

```
apps/
  vios-ockam-service/
    Cargo.toml         # Dependencies: ockam, ockam_core, ockam_vault
    src/
      lib.rs           # OckamService struct
      node.rs          # Ockam node initialization
      transport.rs     # Bridge to vios-driver-network
```

## 4. Security Considerations

### Identity Management
- Each ViOS instance gets a unique Ockam Identity (stored in `/etc/ockam/identity`)
- Use hardware-backed keys if available (TPM/Secure Enclave)

### Trust Model
- Mutual authentication required for all channels
- No plaintext communication allowed between devices

## 5. Next Steps

1. **Create `vios-ockam-service` crate**
2. **Add Ockam dependencies** (check `no_std` compatibility)
3. **Implement minimal node** with identity creation
4. **Test locally** (loopback encrypted channel)
