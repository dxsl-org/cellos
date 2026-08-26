---
title: "X-5 Phase 02 — Cargo + disk + test wiring"
status: ready
effort: 0.5h
---

# Phase 02 — Wiring

## 1. `cells/apps/net-tools/Cargo.toml` — add `[[bin]]`

```toml
[[bin]]
name = "mqtt"
path = "src/bin/mqtt.rs"
```

## 2. `gen_disk.ps1` — two line additions

```powershell
# near line 54 (after $httpd_bin):
$mqtt_bin   = "$rel_dir\mqtt"             # Phase X-5: MQTT client

# near line 140 (after httpd entry):
if (Test-Path $mqtt_bin)  { $table_args += "/bin/mqtt=$mqtt_bin" }
```

## 3. `tests/integration/src/lib.rs` — `spawn_mqtt_broker()`

Single function handles both publish and subscribe tests.
Returns `(port, sender)` where `sender: Sender<Vec<u8>>` receives the first PUBLISH payload.

```rust
/// Spawn a minimal MQTT 3.1.1 mock broker on an ephemeral port.
///
/// Protocol: reads bytes until it sees the CONNECT packet (0x10 first byte),
/// sends CONNACK [0x20 0x02 0x00 0x00], then if a SUBSCRIBE arrives (0x82)
/// sends SUBACK and injects one PUBLISH with `inject_payload`.
/// Returns (port, received_publish_payload).
pub fn spawn_mqtt_broker(inject_payload: &'static [u8])
    -> (u16, std::sync::mpsc::Receiver<Vec<u8>>)
{
    use std::sync::mpsc;
    let listener = TcpListener::bind("127.0.0.1:0").expect("mqtt broker bind");
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else { return };
        let mut buf = [0u8; 512];
        // Read until we have CONNECT (first byte 0x10).
        let mut total = 0usize;
        loop {
            match stream.read(&mut buf[total..]) {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    total += n;
                    if total >= 2 && buf[0] == 0x10 { break; }
                    if total >= 512 { return; }
                }
            }
        }
        // Send CONNACK.
        let _ = stream.write_all(&[0x20, 0x02, 0x00, 0x00]);

        // Read next packet.
        let mut next = [0u8; 512];
        let mut n = 0usize;
        loop {
            match stream.read(&mut next[n..]) {
                Ok(0) | Err(_) => break,
                Ok(k) => {
                    n += k;
                    if n >= 2 { break; }
                }
            }
        }

        if n > 0 && next[0] == 0x82 {
            // SUBSCRIBE: send SUBACK [0x90 0x03 0x00 0x01 0x00].
            let _ = stream.write_all(&[0x90, 0x03, 0x00, 0x01, 0x00]);
            // Inject a PUBLISH with the test payload.
            // Topic = "t" (1 byte), payload = inject_payload.
            let topic = b"t";
            let remaining = 2 + topic.len() + inject_payload.len();
            let mut pkt = vec![0x30u8, remaining as u8,
                               0x00, topic.len() as u8];
            pkt.extend_from_slice(topic);
            pkt.extend_from_slice(inject_payload);
            let _ = stream.write_all(&pkt);
        } else if n > 0 && next[0] == 0x30 {
            // PUBLISH received: capture payload for assertion.
            // remaining_len is next[1]; topic_len is next[3] (big-endian at [2..4]).
            let payload_start = 4 + next[3] as usize; // skip hdr(2)+topiclen(2)+topic
            let rem = next[1] as usize;
            let payload_end = (2 + rem).min(n);
            let _ = tx.send(next[payload_start..payload_end].to_vec());
        }
    });
    (port, rx)
}
```

## 4. `tests/integration/tests/boot.rs` — two test functions

### `mqtt_publish`

```rust
/// Phase X-5: `mqtt publish` sends CONNECT+PUBLISH to a mock broker.
#[test]
fn mqtt_publish() {
    if !prerequisites_ok() { return; }
    let (port, rx) = spawn_mqtt_broker(b"");
    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("prompt: {e}\n{}", qemu.dump()));
    qemu.wait_for("DHCP acquired", 40)
        .unwrap_or_else(|e| panic!("DHCP: {e}\n{}", qemu.dump()));
    std::thread::sleep(Duration::from_millis(300));
    qemu.send_line(&format!("mqtt publish 10.0.2.2:{port} test/topic hello"));
    qemu.wait_for("published", 20)
        .unwrap_or_else(|e| panic!("mqtt publish: {e}\n{}", qemu.dump()));
    // Verify broker received the payload.
    let payload = rx.recv_timeout(Duration::from_secs(5))
        .unwrap_or_default();
    assert!(payload.windows(5).any(|w| w == b"hello"),
        "broker did not receive 'hello', got: {payload:?}");
}
```

### `mqtt_subscribe`

```rust
/// Phase X-5: `mqtt subscribe` receives an injected PUBLISH from the mock broker.
#[test]
fn mqtt_subscribe() {
    if !prerequisites_ok() { return; }
    let (port, _rx) = spawn_mqtt_broker(b"BROKER_MSG");
    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("prompt: {e}\n{}", qemu.dump()));
    qemu.wait_for("DHCP acquired", 40)
        .unwrap_or_else(|e| panic!("DHCP: {e}\n{}", qemu.dump()));
    std::thread::sleep(Duration::from_millis(300));
    qemu.send_line(&format!("mqtt subscribe 10.0.2.2:{port} test/topic"));
    qemu.wait_for("subscribed", 20)
        .unwrap_or_else(|e| panic!("mqtt subscribe: {e}\n{}", qemu.dump()));
    qemu.wait_for("BROKER_MSG", 15)
        .unwrap_or_else(|e| panic!("mqtt payload not received: {e}\n{}", qemu.dump()));
}
```

## Todo

- [ ] Add `[[bin]] mqtt` to Cargo.toml
- [ ] Add `$mqtt_bin` var + table entry to gen_disk.ps1
- [ ] Add `spawn_mqtt_broker()` to tests/integration/src/lib.rs
- [ ] Add `mqtt_publish` test to boot.rs
- [ ] Add `mqtt_subscribe` test to boot.rs
- [ ] `cargo build --release -p app-net-tools`
- [ ] Rebuild disk, run both tests
