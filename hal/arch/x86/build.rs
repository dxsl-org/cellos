use std::{env, fmt::Write as _, fs, path::PathBuf};

const ERROR_VECTORS: [u8; 10] = [8, 10, 11, 12, 13, 14, 17, 21, 29, 30];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo did not set OUT_DIR"));

    let mut asm = String::from(".section .text.x86_idt,\"ax\",@progbits\n");
    for vector in 0u16..=255 {
        writeln!(asm, ".balign 16").unwrap();
        writeln!(asm, ".global x86_64_idt_vector_{vector}").unwrap();
        writeln!(asm, ".type x86_64_idt_vector_{vector},@function").unwrap();
        writeln!(asm, "x86_64_idt_vector_{vector}:").unwrap();
        writeln!(asm, "    .byte 0xf3,0x0f,0x1e,0xfa").unwrap();
        if !ERROR_VECTORS.contains(&(vector as u8)) {
            writeln!(asm, "    pushq $0").unwrap();
        }
        writeln!(asm, "    pushq ${vector}").unwrap();
        writeln!(asm, "    jmp x86_64_idt_common").unwrap();
        writeln!(
            asm,
            ".size x86_64_idt_vector_{vector},.-x86_64_idt_vector_{vector}"
        )
        .unwrap();
    }
    asm.push_str(".section .data.rel.ro.x86_idt,\"aw\",@progbits\n.balign 8\n");
    asm.push_str(".global x86_64_idt_stub_table\n.type x86_64_idt_stub_table,@object\n");
    asm.push_str("x86_64_idt_stub_table:\n");
    for vector in 0u16..=255 {
        writeln!(asm, "    .quad x86_64_idt_vector_{vector}").unwrap();
    }
    asm.push_str(".size x86_64_idt_stub_table,.-x86_64_idt_stub_table\n");
    fs::write(out.join("x86_idt_stubs.S"), asm).expect("write generated IDT assembly");

    let errors = ERROR_VECTORS
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let generated = format!(
        "pub const X86_IDT_STUB_COUNT: usize = 256;\n\
         pub const X86_IDT_ERROR_VECTORS: [u8; 10] = [{errors}];\n"
    );
    fs::write(out.join("x86_idt_generated.rs"), generated).expect("write generated IDT constants");
}
