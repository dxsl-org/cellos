.code16
.global _start
_start:
  cli
  xor %ax,%ax
  mov %ax,%ds
  movw $1,0x8000
write_site:
  addw $2,0x8000
post_site:
  jmp next_site
  nop
next_site:
  nop
  hlt
  jmp next_site
.org 510
.word 0xaa55
