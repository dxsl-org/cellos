
/tmp/cellbox-s10-3d-resume/cand/desktop/files/terminal:     file format elf64-x86-64


Disassembly of section .text:

000000000001da80 <<terminal::sink::ByteSink>::feed>:
   1da80:	55                   	push   %rbp
   1da81:	41 57                	push   %r15
   1da83:	41 56                	push   %r14
   1da85:	41 55                	push   %r13
   1da87:	41 54                	push   %r12
   1da89:	53                   	push   %rbx
   1da8a:	48 81 ec d8 00 00 00 	sub    $0xd8,%rsp
   1da91:	48 89 cb             	mov    %rcx,%rbx
   1da94:	49 89 d6             	mov    %rdx,%r14
   1da97:	49 89 fc             	mov    %rdi,%r12
   1da9a:	48 01 d3             	add    %rdx,%rbx
   1da9d:	48 8d 86 a0 00 00 00 	lea    0xa0(%rsi),%rax
   1daa4:	48 89 44 24 60       	mov    %rax,0x60(%rsp)
   1daa9:	48 8d 86 c8 00 00 00 	lea    0xc8(%rsi),%rax
   1dab0:	48 89 44 24 68       	mov    %rax,0x68(%rsp)
   1dab5:	48 8d 46 50          	lea    0x50(%rsi),%rax
   1dab9:	48 89 84 24 b8 00 00 	mov    %rax,0xb8(%rsp)
   1dac0:	00 
   1dac1:	48 8d 46 60          	lea    0x60(%rsi),%rax
   1dac5:	48 89 84 24 d0 00 00 	mov    %rax,0xd0(%rsp)
   1dacc:	00 
   1dacd:	48 8d 46 09          	lea    0x9(%rsi),%rax
   1dad1:	48 89 84 24 c8 00 00 	mov    %rax,0xc8(%rsp)
   1dad8:	00 
   1dad9:	48 89 34 24          	mov    %rsi,(%rsp)
   1dadd:	48 8d 46 18          	lea    0x18(%rsi),%rax
   1dae1:	48 89 84 24 c0 00 00 	mov    %rax,0xc0(%rsp)
   1dae8:	00 
   1dae9:	48 8d 2d 48 3f 01 00 	lea    0x13f48(%rip),%rbp        # 31a38 <fminimum_numf+0x29b8>
   1daf0:	eb 29                	jmp    1db1b <<terminal::sink::ByteSink>::feed+0x9b>
   1daf2:	41 c6 44 24 3c 02    	movb   $0x2,0x3c(%r12)
   1daf8:	49 c7 04 24 01 00 00 	movq   $0x1,(%r12)
   1daff:	00 
   1db00:	4c 89 e7             	mov    %r12,%rdi
   1db03:	48 8b 34 24          	mov    (%rsp),%rsi
   1db07:	44 89 ea             	mov    %r13d,%edx
   1db0a:	e8 a1 0b 00 00       	call   1e6b0 <<terminal::sink::ByteSink>::escape>
   1db0f:	49 ff c6             	inc    %r14
   1db12:	49 39 de             	cmp    %rbx,%r14
   1db15:	0f 84 c7 0a 00 00    	je     1e5e2 <<terminal::sink::ByteSink>::feed+0xb62>
   1db1b:	45 0f b6 3e          	movzbl (%r14),%r15d
   1db1f:	45                   	rex.RB
