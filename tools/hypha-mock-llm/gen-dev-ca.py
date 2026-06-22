#!/usr/bin/env python3
"""Generate the Cellos dev CA and mock-LLM server certificate.

Outputs (all in the same directory as this script):
  ca.pem      — CA certificate (PEM, commit to repo)
  ca-key.pem  — CA private key (PEM, commit for dev/test use only)
  cert.pem    — server cert signed by the CA (PEM, for mock_proxy.py, gitignored)
  key.pem     — server private key (PEM, for mock_proxy.py, gitignored)

Also writes the CA cert in DER format to:
  ../../cells/services/net/roots/private.der

Idempotent CA: if ca.pem + ca-key.pem already exist the CA is REUSED and only
the server cert is regenerated.  This means anyone can run this script after
`git clone` to get a fresh cert.pem/key.pem that is signed by the same CA
already embedded in the net service binary (private.der).

Run with --force-ca to generate a completely new CA (then also rebuild
service-net and gen_disk.ps1):
  python tools/hypha-mock-llm/gen-dev-ca.py --force-ca
  cargo build --release -p service-net
  ./gen_disk.ps1
"""

import datetime
import ipaddress
import os
import sys

try:
    from cryptography import x509
    from cryptography.x509.oid import NameOID
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import ec
except ImportError:
    sys.exit(
        "Missing `cryptography` package.\n"
        "Install it with: pip install cryptography"
    )

HERE = os.path.dirname(os.path.abspath(__file__))
ROOTS = os.path.join(HERE, "..", "..", "cells", "services", "net", "roots")

ca_pem_path = os.path.join(HERE, "ca.pem")
ca_key_path = os.path.join(HERE, "ca-key.pem")
der_path = os.path.join(ROOTS, "private.der")
cert_path = os.path.join(HERE, "cert.pem")
key_path = os.path.join(HERE, "key.pem")

force_ca = "--force-ca" in sys.argv
now = datetime.datetime.now(datetime.timezone.utc)
TEN_YEARS = datetime.timedelta(days=3650)

# ── CA ────────────────────────────────────────────────────────────────────────

ca_exists = os.path.exists(ca_pem_path) and os.path.exists(ca_key_path)

if ca_exists and not force_ca:
    print("[gen-dev-ca] Reusing existing CA (ca.pem + ca-key.pem) ...")
    with open(ca_pem_path, "rb") as f:
        ca_cert = x509.load_pem_x509_certificate(f.read())
    with open(ca_key_path, "rb") as f:
        ca_key = serialization.load_pem_private_key(f.read(), password=None)
    ca_name = ca_cert.subject
else:
    print("[gen-dev-ca] Generating Cellos dev CA (ECDSA P-256) ...")
    ca_key = ec.generate_private_key(ec.SECP256R1())
    ca_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Cellos Dev CA")])

    ca_cert = (
        x509.CertificateBuilder()
        .subject_name(ca_name)
        .issuer_name(ca_name)
        .public_key(ca_key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(days=1))
        .not_valid_after(now + TEN_YEARS)
        .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
        .add_extension(
            x509.KeyUsage(
                digital_signature=True, key_cert_sign=True, crl_sign=True,
                content_commitment=False, key_encipherment=False,
                data_encipherment=False, key_agreement=False,
                encipher_only=False, decipher_only=False,
            ),
            critical=True,
        )
        .sign(ca_key, hashes.SHA256())
    )

    with open(ca_pem_path, "wb") as f:
        f.write(ca_cert.public_bytes(serialization.Encoding.PEM))
    print(f"  wrote {ca_pem_path}")

    with open(ca_key_path, "wb") as f:
        f.write(ca_key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        ))
    print(f"  wrote {ca_key_path}")

    with open(der_path, "wb") as f:
        f.write(ca_cert.public_bytes(serialization.Encoding.DER))
    print(f"  wrote {der_path}")
    print("[gen-dev-ca] NOTE: rebuild service-net + gen_disk.ps1 to embed the new CA.")

# ── Server cert ────────────────────────────────────────────────────────────────

print("[gen-dev-ca] Generating mock-LLM server cert (signed by dev CA) ...")

server_key = ec.generate_private_key(ec.SECP256R1())
server_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "mock")])

server_cert = (
    x509.CertificateBuilder()
    .subject_name(server_name)
    .issuer_name(ca_name)
    .public_key(server_key.public_key())
    .serial_number(x509.random_serial_number())
    .not_valid_before(now - datetime.timedelta(days=1))
    .not_valid_after(now + TEN_YEARS)
    .add_extension(
        x509.SubjectAlternativeName([
            # "mock" — the SNI the smoke cell sends (HOSTNAME = "mock")
            x509.DNSName("mock"),
            # IP SAN so openssl s_client / curl -k also works
            x509.IPAddress(ipaddress.ip_address("10.0.2.2")),
        ]),
        critical=False,
    )
    .add_extension(
        x509.ExtendedKeyUsage([x509.oid.ExtendedKeyUsageOID.SERVER_AUTH]),
        critical=False,
    )
    .sign(ca_key, hashes.SHA256())
)

with open(cert_path, "wb") as f:
    f.write(server_cert.public_bytes(serialization.Encoding.PEM))
print(f"  wrote {cert_path}")

with open(key_path, "wb") as f:
    f.write(server_key.private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    ))
print(f"  wrote {key_path}")

print()
print("[gen-dev-ca] Done.")
if not (ca_exists and not force_ca):
    print("  New CA was generated — rebuild before testing:")
    print("    cargo build --release -p service-net")
    print("    ./gen_disk.ps1")
else:
    print("  Existing CA reused — no rebuild needed.")
    print("  Restart mock_proxy.py to pick up the new cert.pem/key.pem.")
