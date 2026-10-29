
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
   1db1f:	45 0f b6 ef          	movzbl %r15b,%r13d
   1db23:	41 0f b6 44 24 3c    	movzbl 0x3c(%r12),%eax
   1db29:	48 63 44 85 00       	movslq 0x0(%rbp,%rax,4),%rax
   1db2e:	48 01 e8             	add    %rbp,%rax
   1db31:	ff e0                	jmp    *%rax
   1db33:	41 80 ff c0          	cmp    $0xc0,%r15b
   1db37:	0f 8d 3e 01 00 00    	jge    1dc7b <<terminal::sink::ByteSink>::feed+0x1fb>
   1db3d:	41 0f b6 7c 24 3e    	movzbl 0x3e(%r12),%edi
   1db43:	40 80 ff 03          	cmp    $0x3,%dil
   1db47:	0f 87 a7 0a 00 00    	ja     1e5f4 <<terminal::sink::ByteSink>::feed+0xb74>
   1db4d:	41 0f b6 44 24 3d    	movzbl 0x3d(%r12),%eax
   1db53:	49 8d 74 24 38       	lea    0x38(%r12),%rsi
   1db58:	44 88 3c 3e          	mov    %r15b,(%rsi,%rdi,1)
   1db5c:	8d 4f 01             	lea    0x1(%rdi),%ecx
   1db5f:	38 c1                	cmp    %al,%cl
   1db61:	0f 85 5f 01 00 00    	jne    1dcc6 <<terminal::sink::ByteSink>::feed+0x246>
   1db67:	0f b6 d0             	movzbl %al,%edx
   1db6a:	41 c6 44 24 3c 00    	movb   $0x0,0x3c(%r12)
   1db70:	48 8d 7c 24 10       	lea    0x10(%rsp),%rdi
   1db75:	e8 56 b2 fe ff       	call   8dd0 <core::str::converts::from_utf8>
   1db7a:	83 7c 24 10 01       	cmpl   $0x1,0x10(%rsp)
   1db7f:	0f 85 be 01 00 00    	jne    1dd43 <<terminal::sink::ByteSink>::feed+0x2c3>
   1db85:	be fd ff 00 00       	mov    $0xfffd,%esi
   1db8a:	48 8b 3c 24          	mov    (%rsp),%rdi
   1db8e:	e8 7d 68 00 00       	call   24410 <<terminal::grid::TerminalGrid>::print>
   1db93:	e9 77 ff ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1db98:	41 8d 47 e0          	lea    -0x20(%r15),%eax
   1db9c:	3c 5f                	cmp    $0x5f,%al
   1db9e:	0f 83 96 00 00 00    	jae    1dc3a <<terminal::sink::ByteSink>::feed+0x1ba>
   1dba4:	49 8b 04 24          	mov    (%r12),%rax
   1dba8:	48 3d ff 00 00 00    	cmp    $0xff,%rax
   1dbae:	0f 87 e1 01 00 00    	ja     1dd95 <<terminal::sink::ByteSink>::feed+0x315>
   1dbb4:	48 ff c0             	inc    %rax
   1dbb7:	49 89 04 24          	mov    %rax,(%r12)
   1dbbb:	41 8d 4f d0          	lea    -0x30(%r15),%ecx
   1dbbf:	80 f9 0a             	cmp    $0xa,%cl
   1dbc2:	0f 83 2f 01 00 00    	jae    1dcf7 <<terminal::sink::ByteSink>::feed+0x277>
   1dbc8:	49 8b 74 24 28       	mov    0x28(%r12),%rsi
   1dbcd:	48 85 f6             	test   %rsi,%rsi
   1dbd0:	0f 84 d9 01 00 00    	je     1ddaf <<terminal::sink::ByteSink>::feed+0x32f>
   1dbd6:	48 83 fe 10          	cmp    $0x10,%rsi
   1dbda:	0f 87 2f ff ff ff    	ja     1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dbe0:	48 ff ce             	dec    %rsi
   1dbe3:	e9 d2 01 00 00       	jmp    1ddba <<terminal::sink::ByteSink>::feed+0x33a>
   1dbe8:	44 89 f8             	mov    %r15d,%eax
   1dbeb:	24 f0                	and    $0xf0,%al
   1dbed:	3c 20                	cmp    $0x20,%al
   1dbef:	75 6a                	jne    1dc5b <<terminal::sink::ByteSink>::feed+0x1db>
   1dbf1:	49 8b 04 24          	mov    (%r12),%rax
   1dbf5:	48 3d ff 00 00 00    	cmp    $0xff,%rax
   1dbfb:	0f 86 ea 00 00 00    	jbe    1dceb <<terminal::sink::ByteSink>::feed+0x26b>
   1dc01:	e9 8f 01 00 00       	jmp    1dd95 <<terminal::sink::ByteSink>::feed+0x315>
   1dc06:	41 8d 45 f9          	lea    -0x7(%r13),%eax
   1dc0a:	83 f8 14             	cmp    $0x14,%eax
   1dc0d:	0f 87 c8 00 00 00    	ja     1dcdb <<terminal::sink::ByteSink>::feed+0x25b>
   1dc13:	48 8d 0d 3a 3e 01 00 	lea    0x13e3a(%rip),%rcx        # 31a54 <fminimum_numf+0x29d4>
   1dc1a:	48 63 04 81          	movslq (%rcx,%rax,4),%rax
   1dc1e:	48 01 c8             	add    %rcx,%rax
   1dc21:	ff e0                	jmp    *%rax
   1dc23:	41 c6 44 24 3c 06    	movb   $0x6,0x3c(%r12)
   1dc29:	e9 e1 fe ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dc2e:	41 80 ff 5c          	cmp    $0x5c,%r15b
   1dc32:	0f 85 ba fe ff ff    	jne    1daf2 <<terminal::sink::ByteSink>::feed+0x72>
   1dc38:	eb 36                	jmp    1dc70 <<terminal::sink::ByteSink>::feed+0x1f0>
   1dc3a:	41 83 fd 18          	cmp    $0x18,%r13d
   1dc3e:	74 30                	je     1dc70 <<terminal::sink::ByteSink>::feed+0x1f0>
   1dc40:	41 83 fd 1a          	cmp    $0x1a,%r13d
   1dc44:	74 2a                	je     1dc70 <<terminal::sink::ByteSink>::feed+0x1f0>
   1dc46:	41 83 fd 1b          	cmp    $0x1b,%r13d
   1dc4a:	74 67                	je     1dcb3 <<terminal::sink::ByteSink>::feed+0x233>
   1dc4c:	41 80 ff 20          	cmp    $0x20,%r15b
   1dc50:	0f 82 24 01 00 00    	jb     1dd7a <<terminal::sink::ByteSink>::feed+0x2fa>
   1dc56:	e9 30 01 00 00       	jmp    1dd8b <<terminal::sink::ByteSink>::feed+0x30b>
   1dc5b:	41 8d 47 d0          	lea    -0x30(%r15),%eax
   1dc5f:	3c 4f                	cmp    $0x4f,%al
   1dc61:	73 3a                	jae    1dc9d <<terminal::sink::ByteSink>::feed+0x21d>
   1dc63:	66 66 66 66 2e 0f 1f 	data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
   1dc6a:	84 00 00 00 00 00 
   1dc70:	41 c6 44 24 3c 00    	movb   $0x0,0x3c(%r12)
   1dc76:	e9 94 fe ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dc7b:	41 c6 44 24 3c 00    	movb   $0x0,0x3c(%r12)
   1dc81:	4c 8b 3c 24          	mov    (%rsp),%r15
   1dc85:	4c 89 ff             	mov    %r15,%rdi
   1dc88:	be fd ff 00 00       	mov    $0xfffd,%esi
   1dc8d:	e8 7e 67 00 00       	call   24410 <<terminal::grid::TerminalGrid>::print>
   1dc92:	4c 89 e7             	mov    %r12,%rdi
   1dc95:	4c 89 fe             	mov    %r15,%rsi
   1dc98:	e9 05 01 00 00       	jmp    1dda2 <<terminal::sink::ByteSink>::feed+0x322>
   1dc9d:	41 83 fd 18          	cmp    $0x18,%r13d
   1dca1:	74 cd                	je     1dc70 <<terminal::sink::ByteSink>::feed+0x1f0>
   1dca3:	41 83 fd 1a          	cmp    $0x1a,%r13d
   1dca7:	74 c7                	je     1dc70 <<terminal::sink::ByteSink>::feed+0x1f0>
   1dca9:	41 83 fd 1b          	cmp    $0x1b,%r13d
   1dcad:	0f 85 bb 00 00 00    	jne    1dd6e <<terminal::sink::ByteSink>::feed+0x2ee>
   1dcb3:	41 c6 44 24 3c 02    	movb   $0x2,0x3c(%r12)
   1dcb9:	49 c7 04 24 01 00 00 	movq   $0x1,(%r12)
   1dcc0:	00 
   1dcc1:	e9 49 fe ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dcc6:	41 c6 44 24 3c 01    	movb   $0x1,0x3c(%r12)
   1dccc:	41 88 44 24 3d       	mov    %al,0x3d(%r12)
   1dcd1:	41 88 4c 24 3e       	mov    %cl,0x3e(%r12)
   1dcd6:	e9 34 fe ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dcdb:	49 8b 04 24          	mov    (%r12),%rax
   1dcdf:	48 3d ff 0f 00 00    	cmp    $0xfff,%rax
   1dce5:	0f 87 aa 00 00 00    	ja     1dd95 <<terminal::sink::ByteSink>::feed+0x315>
   1dceb:	48 ff c0             	inc    %rax
   1dcee:	49 89 04 24          	mov    %rax,(%r12)
   1dcf2:	e9 18 fe ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dcf7:	41 83 fd 3a          	cmp    $0x3a,%r13d
   1dcfb:	0f 84 5d 01 00 00    	je     1de5e <<terminal::sink::ByteSink>::feed+0x3de>
   1dd01:	41 83 fd 3b          	cmp    $0x3b,%r13d
   1dd05:	0f 85 ed 00 00 00    	jne    1ddf8 <<terminal::sink::ByteSink>::feed+0x378>
   1dd0b:	49 8b 44 24 28       	mov    0x28(%r12),%rax
   1dd10:	48 83 f8 01          	cmp    $0x1,%rax
   1dd14:	48 83 d0 00          	adc    $0x0,%rax
   1dd18:	48 ff c0             	inc    %rax
   1dd1b:	48 c7 c1 ff ff ff ff 	mov    $0xffffffffffffffff,%rcx
   1dd22:	48 0f 44 c1          	cmove  %rcx,%rax
   1dd26:	48 83 f8 11          	cmp    $0x11,%rax
   1dd2a:	b9 11 00 00 00       	mov    $0x11,%ecx
   1dd2f:	48 0f 43 c1          	cmovae %rcx,%rax
   1dd33:	49 89 44 24 28       	mov    %rax,0x28(%r12)
   1dd38:	0f 82 d1 fd ff ff    	jb     1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dd3e:	e9 1b 01 00 00       	jmp    1de5e <<terminal::sink::ByteSink>::feed+0x3de>
   1dd43:	48 83 7c 24 20 00    	cmpq   $0x0,0x20(%rsp)
   1dd49:	48 8b 3c 24          	mov    (%rsp),%rdi
   1dd4d:	0f 84 96 00 00 00    	je     1dde9 <<terminal::sink::ByteSink>::feed+0x369>
   1dd53:	48 8b 4c 24 18       	mov    0x18(%rsp),%rcx
   1dd58:	0f b6 31             	movzbl (%rcx),%esi
   1dd5b:	40 84 f6             	test   %sil,%sil
   1dd5e:	0f 88 af 00 00 00    	js     1de13 <<terminal::sink::ByteSink>::feed+0x393>
   1dd64:	e8 a7 66 00 00       	call   24410 <<terminal::grid::TerminalGrid>::print>
   1dd69:	e9 a1 fd ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dd6e:	41 80 ff 20          	cmp    $0x20,%r15b
   1dd72:	73 17                	jae    1dd8b <<terminal::sink::ByteSink>::feed+0x30b>
   1dd74:	41 c6 44 24 3c 00    	movb   $0x0,0x3c(%r12)
   1dd7a:	48 8b 3c 24          	mov    (%rsp),%rdi
   1dd7e:	44 89 ee             	mov    %r13d,%esi
   1dd81:	e8 6a 0b 00 00       	call   1e8f0 <terminal::sink::control>
   1dd86:	e9 84 fd ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dd8b:	41 80 ff 7f          	cmp    $0x7f,%r15b
   1dd8f:	0f 84 7a fd ff ff    	je     1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dd95:	41 c6 44 24 3c 00    	movb   $0x0,0x3c(%r12)
   1dd9b:	4c 89 e7             	mov    %r12,%rdi
   1dd9e:	48 8b 34 24          	mov    (%rsp),%rsi
   1dda2:	44 89 ea             	mov    %r13d,%edx
   1dda5:	e8 76 08 00 00       	call   1e620 <<terminal::sink::ByteSink>::ground>
   1ddaa:	e9 60 fd ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1ddaf:	49 c7 44 24 28 01 00 	movq   $0x1,0x28(%r12)
   1ddb6:	00 00 
   1ddb8:	31 f6                	xor    %esi,%esi
   1ddba:	66 b8 0a 00          	mov    $0xa,%ax
   1ddbe:	49 8d 54 24 08       	lea    0x8(%r12),%rdx
   1ddc3:	66 f7 24 72          	mulw   (%rdx,%rsi,2)
   1ddc7:	0f 80 61 07 00 00    	jo     1e52e <<terminal::sink::ByteSink>::feed+0xaae>
   1ddcd:	0f b6 c9             	movzbl %cl,%ecx
   1ddd0:	66 01 c1             	add    %ax,%cx
   1ddd3:	b8 ff ff 00 00       	mov    $0xffff,%eax
   1ddd8:	0f 42 c8             	cmovb  %eax,%ecx
   1dddb:	49 8d 44 24 08       	lea    0x8(%r12),%rax
   1dde0:	66 89 0c 70          	mov    %cx,(%rax,%rsi,2)
   1dde4:	e9 26 fd ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dde9:	be fd ff 00 00       	mov    $0xfffd,%esi
   1ddee:	e8 1d 66 00 00       	call   24410 <<terminal::grid::TerminalGrid>::print>
   1ddf3:	e9 17 fd ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1ddf8:	44 89 f9             	mov    %r15d,%ecx
   1ddfb:	80 e1 7c             	and    $0x7c,%cl
   1ddfe:	80 f9 3c             	cmp    $0x3c,%cl
   1de01:	75 55                	jne    1de58 <<terminal::sink::ByteSink>::feed+0x3d8>
   1de03:	48 83 f8 03          	cmp    $0x3,%rax
   1de07:	75 55                	jne    1de5e <<terminal::sink::ByteSink>::feed+0x3de>
   1de09:	45 88 7c 24 30       	mov    %r15b,0x30(%r12)
   1de0e:	e9 fc fc ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1de13:	89 f0                	mov    %esi,%eax
   1de15:	83 e0 1f             	and    $0x1f,%eax
   1de18:	44 0f b6 41 01       	movzbl 0x1(%rcx),%r8d
   1de1d:	41 83 e0 3f          	and    $0x3f,%r8d
   1de21:	40 80 fe df          	cmp    $0xdf,%sil
   1de25:	76 42                	jbe    1de69 <<terminal::sink::ByteSink>::feed+0x3e9>
   1de27:	0f b6 51 02          	movzbl 0x2(%rcx),%edx
   1de2b:	41 c1 e0 06          	shl    $0x6,%r8d
   1de2f:	83 e2 3f             	and    $0x3f,%edx
   1de32:	44 09 c2             	or     %r8d,%edx
   1de35:	40 80 fe f0          	cmp    $0xf0,%sil
   1de39:	0f 82 fa 00 00 00    	jb     1df39 <<terminal::sink::ByteSink>::feed+0x4b9>
   1de3f:	0f b6 71 03          	movzbl 0x3(%rcx),%esi
   1de43:	83 e0 07             	and    $0x7,%eax
   1de46:	c1 e0 12             	shl    $0x12,%eax
   1de49:	c1 e2 06             	shl    $0x6,%edx
   1de4c:	83 e6 3f             	and    $0x3f,%esi
   1de4f:	09 d6                	or     %edx,%esi
   1de51:	09 c6                	or     %eax,%esi
   1de53:	e9 0c ff ff ff       	jmp    1dd64 <<terminal::sink::ByteSink>::feed+0x2e4>
   1de58:	41 80 ff 30          	cmp    $0x30,%r15b
   1de5c:	73 1d                	jae    1de7b <<terminal::sink::ByteSink>::feed+0x3fb>
   1de5e:	41 c6 44 24 31 01    	movb   $0x1,0x31(%r12)
   1de64:	e9 a6 fc ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1de69:	c1 e0 06             	shl    $0x6,%eax
   1de6c:	44 09 c0             	or     %r8d,%eax
   1de6f:	89 c6                	mov    %eax,%esi
   1de71:	e8 9a 65 00 00       	call   24410 <<terminal::grid::TerminalGrid>::print>
   1de76:	e9 94 fc ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1de7b:	41 c6 44 24 3c 00    	movb   $0x0,0x3c(%r12)
   1de81:	41 80 7c 24 31 00    	cmpb   $0x0,0x31(%r12)
   1de87:	0f 85 82 fc ff ff    	jne    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1de8d:	49 8d 74 24 08       	lea    0x8(%r12),%rsi
   1de92:	48 8b 4e 28          	mov    0x28(%rsi),%rcx
   1de96:	48 89 4c 24 38       	mov    %rcx,0x38(%rsp)
   1de9b:	48 8b 46 20          	mov    0x20(%rsi),%rax
   1de9f:	48 89 44 24 30       	mov    %rax,0x30(%rsp)
   1dea4:	48 8b 56 18          	mov    0x18(%rsi),%rdx
   1dea8:	48 89 54 24 28       	mov    %rdx,0x28(%rsp)
   1dead:	48 8b 56 10          	mov    0x10(%rsi),%rdx
   1deb1:	48 89 54 24 20       	mov    %rdx,0x20(%rsp)
   1deb6:	48 8b 16             	mov    (%rsi),%rdx
   1deb9:	48 8b 76 08          	mov    0x8(%rsi),%rsi
   1debd:	48 89 74 24 18       	mov    %rsi,0x18(%rsp)
   1dec2:	48 89 54 24 10       	mov    %rdx,0x10(%rsp)
   1dec7:	48 83 f8 10          	cmp    $0x10,%rax
   1decb:	ba 10 00 00 00       	mov    $0x10,%edx
   1ded0:	48 0f 42 d0          	cmovb  %rax,%rdx
   1ded4:	0f b6 c9             	movzbl %cl,%ecx
   1ded7:	83 f9 3f             	cmp    $0x3f,%ecx
   1deda:	74 6e                	je     1df4a <<terminal::sink::ByteSink>::feed+0x4ca>
   1dedc:	85 c9                	test   %ecx,%ecx
   1dede:	0f 85 2b fc ff ff    	jne    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1dee4:	41 83 c5 bf          	add    $0xffffffbf,%r13d
   1dee8:	41 83 fd 34          	cmp    $0x34,%r13d
   1deec:	0f 87 1d fc ff ff    	ja     1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1def2:	48 8d 35 af 3b 01 00 	lea    0x13baf(%rip),%rsi        # 31aa8 <fminimum_numf+0x2a28>
   1def9:	4a 63 0c ae          	movslq (%rsi,%r13,4),%rcx
   1defd:	48 01 f1             	add    %rsi,%rcx
   1df00:	ff e1                	jmp    *%rcx
   1df02:	b9 01 00 00 00       	mov    $0x1,%ecx
   1df07:	48 85 c0             	test   %rax,%rax
   1df0a:	0f 84 76 05 00 00    	je     1e486 <<terminal::sink::ByteSink>::feed+0xa06>
   1df10:	8b 54 24 10          	mov    0x10(%rsp),%edx
   1df14:	66 83 fa 01          	cmp    $0x1,%dx
   1df18:	83 d2 00             	adc    $0x0,%edx
   1df1b:	48 83 f8 01          	cmp    $0x1,%rax
   1df1f:	0f 84 65 05 00 00    	je     1e48a <<terminal::sink::ByteSink>::feed+0xa0a>
   1df25:	0f b7 44 24 12       	movzwl 0x12(%rsp),%eax
   1df2a:	66 83 f8 01          	cmp    $0x1,%ax
   1df2e:	83 d0 00             	adc    $0x0,%eax
   1df31:	0f b7 c8             	movzwl %ax,%ecx
   1df34:	e9 51 05 00 00       	jmp    1e48a <<terminal::sink::ByteSink>::feed+0xa0a>
   1df39:	c1 e0 0c             	shl    $0xc,%eax
   1df3c:	09 c2                	or     %eax,%edx
   1df3e:	89 d6                	mov    %edx,%esi
   1df40:	e8 cb 64 00 00       	call   24410 <<terminal::grid::TerminalGrid>::print>
   1df45:	e9 c5 fb ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1df4a:	41 83 cd 04          	or     $0x4,%r13d
   1df4e:	41 83 fd 6c          	cmp    $0x6c,%r13d
   1df52:	0f 85 b7 fb ff ff    	jne    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1df58:	48 85 c0             	test   %rax,%rax
   1df5b:	0f 84 ae fb ff ff    	je     1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1df61:	48 01 d2             	add    %rdx,%rdx
   1df64:	41 80 ff 68          	cmp    $0x68,%r15b
   1df68:	0f 94 c1             	sete   %cl
   1df6b:	45 31 ed             	xor    %r13d,%r13d
   1df6e:	48 89 54 24 58       	mov    %rdx,0x58(%rsp)
   1df73:	88 4c 24 0f          	mov    %cl,0xf(%rsp)
   1df77:	eb 20                	jmp    1df99 <<terminal::sink::ByteSink>::feed+0x519>
   1df79:	48 8b 3c 24          	mov    (%rsp),%rdi
   1df7d:	e8 2e 5d 00 00       	call   23cb0 <<terminal::grid::TerminalGrid>::leave_alternate>
   1df82:	0f b6 4c 24 0f       	movzbl 0xf(%rsp),%ecx
   1df87:	48 8b 54 24 58       	mov    0x58(%rsp),%rdx
   1df8c:	49 83 c5 02          	add    $0x2,%r13
   1df90:	4c 39 ea             	cmp    %r13,%rdx
   1df93:	0f 84 76 fb ff ff    	je     1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1df99:	42 0f b7 44 2c 10    	movzwl 0x10(%rsp,%r13,1),%eax
   1df9f:	3d 18 04 00 00       	cmp    $0x418,%eax
   1dfa4:	7f 16                	jg     1dfbc <<terminal::sink::ByteSink>::feed+0x53c>
   1dfa6:	83 f8 07             	cmp    $0x7,%eax
   1dfa9:	74 2b                	je     1dfd6 <<terminal::sink::ByteSink>::feed+0x556>
   1dfab:	83 f8 19             	cmp    $0x19,%eax
   1dfae:	75 dc                	jne    1df8c <<terminal::sink::ByteSink>::feed+0x50c>
   1dfb0:	48 8b 04 24          	mov    (%rsp),%rax
   1dfb4:	88 88 d5 00 00 00    	mov    %cl,0xd5(%rax)
   1dfba:	eb d0                	jmp    1df8c <<terminal::sink::ByteSink>::feed+0x50c>
   1dfbc:	3d 19 04 00 00       	cmp    $0x419,%eax
   1dfc1:	74 30                	je     1dff3 <<terminal::sink::ByteSink>::feed+0x573>
   1dfc3:	3d d4 07 00 00       	cmp    $0x7d4,%eax
   1dfc8:	75 c2                	jne    1df8c <<terminal::sink::ByteSink>::feed+0x50c>
   1dfca:	48 8b 04 24          	mov    (%rsp),%rax
   1dfce:	88 88 d6 00 00 00    	mov    %cl,0xd6(%rax)
   1dfd4:	eb b6                	jmp    1df8c <<terminal::sink::ByteSink>::feed+0x50c>
   1dfd6:	48 8b 04 24          	mov    (%rsp),%rax
   1dfda:	88 88 d4 00 00 00    	mov    %cl,0xd4(%rax)
   1dfe0:	41 80 ff 68          	cmp    $0x68,%r15b
   1dfe4:	74 a6                	je     1df8c <<terminal::sink::ByteSink>::feed+0x50c>
   1dfe6:	48 8b 04 24          	mov    (%rsp),%rax
   1dfea:	c6 80 d2 00 00 00 00 	movb   $0x0,0xd2(%rax)
   1dff1:	eb 99                	jmp    1df8c <<terminal::sink::ByteSink>::feed+0x50c>
   1dff3:	41 80 ff 68          	cmp    $0x68,%r15b
   1dff7:	75 80                	jne    1df79 <<terminal::sink::ByteSink>::feed+0x4f9>
   1dff9:	48 8b 04 24          	mov    (%rsp),%rax
   1dffd:	80 b8 d3 00 00 00 00 	cmpb   $0x0,0xd3(%rax)
   1e004:	75 86                	jne    1df8c <<terminal::sink::ByteSink>::feed+0x50c>
   1e006:	48 8b 14 24          	mov    (%rsp),%rdx
   1e00a:	48 8b 8a a0 00 00 00 	mov    0xa0(%rdx),%rcx
   1e011:	48 8b b2 a8 00 00 00 	mov    0xa8(%rdx),%rsi
   1e018:	0f b6 82 d2 00 00 00 	movzbl 0xd2(%rdx),%eax
   1e01f:	48 89 8c 24 b0 00 00 	mov    %rcx,0xb0(%rsp)
   1e026:	00 
   1e027:	48 89 4a 50          	mov    %rcx,0x50(%rdx)
   1e02b:	48 89 b4 24 a0 00 00 	mov    %rsi,0xa0(%rsp)
   1e032:	00 
   1e033:	48 89 72 58          	mov    %rsi,0x58(%rdx)
   1e037:	48 8b 74 24 68       	mov    0x68(%rsp),%rsi
   1e03c:	0f b7 4e 08          	movzwl 0x8(%rsi),%ecx
   1e040:	48 8b bc 24 d0 00 00 	mov    0xd0(%rsp),%rdi
   1e047:	00 
   1e048:	66 89 4f 08          	mov    %cx,0x8(%rdi)
   1e04c:	48 8b 0e             	mov    (%rsi),%rcx
   1e04f:	48 89 0f             	mov    %rcx,(%rdi)
   1e052:	88 42 6a             	mov    %al,0x6a(%rdx)
   1e055:	48 8b 82 90 00 00 00 	mov    0x90(%rdx),%rax
   1e05c:	48 8b 8a 98 00 00 00 	mov    0x98(%rdx),%rcx
   1e063:	48 89 84 24 a8 00 00 	mov    %rax,0xa8(%rsp)
   1e06a:	00 
   1e06b:	48 0f af c8          	imul   %rax,%rcx
   1e06f:	48 c7 44 24 40 00 00 	movq   $0x0,0x40(%rsp)
   1e076:	00 00 
   1e078:	48 c7 44 24 48 04 00 	movq   $0x4,0x48(%rsp)
   1e07f:	00 00 
   1e081:	48 c7 44 24 50 00 00 	movq   $0x0,0x50(%rsp)
   1e088:	00 00 
   1e08a:	48 85 c9             	test   %rcx,%rcx
   1e08d:	0f 85 4c 01 00 00    	jne    1e1df <<terminal::sink::ByteSink>::feed+0x75f>
   1e093:	48 8d 7c 24 40       	lea    0x40(%rsp),%rdi
   1e098:	48 89 ce             	mov    %rcx,%rsi
   1e09b:	e8 d0 38 fe ff       	call   1970 <<alloc::vec::Vec<terminal::grid::TerminalCell>>::resize>
   1e0a0:	48 8b 44 24 40       	mov    0x40(%rsp),%rax
   1e0a5:	0f b6 4c 24 48       	movzbl 0x48(%rsp),%ecx
   1e0aa:	48 8d 74 24 49       	lea    0x49(%rsp),%rsi
   1e0af:	48 8b 16             	mov    (%rsi),%rdx
   1e0b2:	48 8b 76 07          	mov    0x7(%rsi),%rsi
   1e0b6:	48 89 94 24 88 00 00 	mov    %rdx,0x88(%rsp)
   1e0bd:	00 
   1e0be:	48 89 b4 24 8f 00 00 	mov    %rsi,0x8f(%rsp)
   1e0c5:	00 
   1e0c6:	48 83 f8 ff          	cmp    $0xffffffffffffffff,%rax
   1e0ca:	0f 84 3b 01 00 00    	je     1e20b <<terminal::sink::ByteSink>::feed+0x78b>
   1e0d0:	48 8b 94 24 88 00 00 	mov    0x88(%rsp),%rdx
   1e0d7:	00 
   1e0d8:	48 8b b4 24 8f 00 00 	mov    0x8f(%rsp),%rsi
   1e0df:	00 
   1e0e0:	48 89 74 24 47       	mov    %rsi,0x47(%rsp)
   1e0e5:	48 89 54 24 40       	mov    %rdx,0x40(%rsp)
   1e0ea:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e0ee:	48 8b 57 10          	mov    0x10(%rdi),%rdx
   1e0f2:	48 89 94 24 80 00 00 	mov    %rdx,0x80(%rsp)
   1e0f9:	00 
   1e0fa:	48 8b 17             	mov    (%rdi),%rdx
   1e0fd:	48 8b 77 08          	mov    0x8(%rdi),%rsi
   1e101:	48 89 74 24 78       	mov    %rsi,0x78(%rsp)
   1e106:	48 89 54 24 70       	mov    %rdx,0x70(%rsp)
   1e10b:	48 89 07             	mov    %rax,(%rdi)
   1e10e:	88 4f 08             	mov    %cl,0x8(%rdi)
   1e111:	48 8b 44 24 40       	mov    0x40(%rsp),%rax
   1e116:	48 8b 4c 24 47       	mov    0x47(%rsp),%rcx
   1e11b:	48 8b 94 24 c8 00 00 	mov    0xc8(%rsp),%rdx
   1e122:	00 
   1e123:	48 89 4a 07          	mov    %rcx,0x7(%rdx)
   1e127:	48 89 02             	mov    %rax,(%rdx)
   1e12a:	48 8b 47 18          	mov    0x18(%rdi),%rax
   1e12e:	48 85 c0             	test   %rax,%rax
   1e131:	74 1e                	je     1e151 <<terminal::sink::ByteSink>::feed+0x6d1>
   1e133:	48 8b 0c 24          	mov    (%rsp),%rcx
   1e137:	48 8b 79 20          	mov    0x20(%rcx),%rdi
   1e13b:	48 c1 e0 02          	shl    $0x2,%rax
   1e13f:	48 8d 14 80          	lea    (%rax,%rax,4),%rdx
   1e143:	be 04 00 00 00       	mov    $0x4,%esi
   1e148:	e8 63 ce fe ff       	call   afb0 <<linked_list_allocator::Heap>::deallocate>
   1e14d:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e151:	48 8b 84 24 80 00 00 	mov    0x80(%rsp),%rax
   1e158:	00 
   1e159:	48 8b 94 24 c0 00 00 	mov    0xc0(%rsp),%rdx
   1e160:	00 
   1e161:	48 89 42 10          	mov    %rax,0x10(%rdx)
   1e165:	48 8b 44 24 70       	mov    0x70(%rsp),%rax
   1e16a:	48 8b 4c 24 78       	mov    0x78(%rsp),%rcx
   1e16f:	48 89 4a 08          	mov    %rcx,0x8(%rdx)
   1e173:	48 89 02             	mov    %rax,(%rdx)
   1e176:	c6 87 d3 00 00 00 01 	movb   $0x1,0xd3(%rdi)
   1e17d:	c6 87 8a 00 00 00 02 	movb   $0x2,0x8a(%rdi)
   1e184:	4c 8b 84 24 b0 00 00 	mov    0xb0(%rsp),%r8
   1e18b:	00 
   1e18c:	4d 85 c0             	test   %r8,%r8
   1e18f:	48 8b 54 24 58       	mov    0x58(%rsp),%rdx
   1e194:	74 3f                	je     1e1d5 <<terminal::sink::ByteSink>::feed+0x755>
   1e196:	48 8b bc 24 a8 00 00 	mov    0xa8(%rsp),%rdi
   1e19d:	00 
   1e19e:	48 0f af bc 24 a0 00 	imul   0xa0(%rsp),%rdi
   1e1a5:	00 00 
   1e1a7:	4c 01 c7             	add    %r8,%rdi
   1e1aa:	48 8b 04 24          	mov    (%rsp),%rax
   1e1ae:	48 8b 70 10          	mov    0x10(%rax),%rsi
   1e1b2:	48 39 f7             	cmp    %rsi,%rdi
   1e1b5:	0f 83 4a 04 00 00    	jae    1e605 <<terminal::sink::ByteSink>::feed+0xb85>
   1e1bb:	48 8b 40 08          	mov    0x8(%rax),%rax
   1e1bf:	48 8d 0c bf          	lea    (%rdi,%rdi,4),%rcx
   1e1c3:	80 7c 88 12 02       	cmpb   $0x2,0x12(%rax,%rcx,4)
   1e1c8:	75 0b                	jne    1e1d5 <<terminal::sink::ByteSink>::feed+0x755>
   1e1ca:	49 ff c8             	dec    %r8
   1e1cd:	48 8b 44 24 60       	mov    0x60(%rsp),%rax
   1e1d2:	4c 89 00             	mov    %r8,(%rax)
   1e1d5:	0f b6 4c 24 0f       	movzbl 0xf(%rsp),%ecx
   1e1da:	e9 ad fd ff ff       	jmp    1df8c <<terminal::sink::ByteSink>::feed+0x50c>
   1e1df:	ba 04 00 00 00       	mov    $0x4,%edx
   1e1e4:	41 b8 04 00 00 00    	mov    $0x4,%r8d
   1e1ea:	41 b9 14 00 00 00    	mov    $0x14,%r9d
   1e1f0:	48 8d 7c 24 70       	lea    0x70(%rsp),%rdi
   1e1f5:	31 f6                	xor    %esi,%esi
   1e1f7:	48 89 8c 24 98 00 00 	mov    %rcx,0x98(%rsp)
   1e1fe:	00 
   1e1ff:	e8 bc 39 fe ff       	call   1bc0 <<alloc::raw_vec::RawVecInner>::finish_grow>
   1e204:	80 7c 24 70 00       	cmpb   $0x0,0x70(%rsp)
   1e209:	74 0f                	je     1e21a <<terminal::sink::ByteSink>::feed+0x79a>
   1e20b:	48 8b 54 24 58       	mov    0x58(%rsp),%rdx
   1e210:	0f b6 4c 24 0f       	movzbl 0xf(%rsp),%ecx
   1e215:	e9 72 fd ff ff       	jmp    1df8c <<terminal::sink::ByteSink>::feed+0x50c>
   1e21a:	48 8b 44 24 78       	mov    0x78(%rsp),%rax
   1e21f:	48 89 44 24 48       	mov    %rax,0x48(%rsp)
   1e224:	48 8b 8c 24 98 00 00 	mov    0x98(%rsp),%rcx
   1e22b:	00 
   1e22c:	48 89 4c 24 40       	mov    %rcx,0x40(%rsp)
   1e231:	e9 5d fe ff ff       	jmp    1e093 <<terminal::sink::ByteSink>::feed+0x613>
   1e236:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e23a:	49 8d 74 24 08       	lea    0x8(%r12),%rsi
   1e23f:	e8 ac 5f 00 00       	call   241f0 <<terminal::grid::TerminalGrid>::select_graphic_rendition>
   1e244:	e9 c6 f8 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e249:	48 83 f8 02          	cmp    $0x2,%rax
   1e24d:	0f 83 bc f8 ff ff    	jae    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e253:	48 85 c0             	test   %rax,%rax
   1e256:	0f 84 60 03 00 00    	je     1e5bc <<terminal::sink::ByteSink>::feed+0xb3c>
   1e25c:	8b 44 24 10          	mov    0x10(%rsp),%eax
   1e260:	66 83 f8 01          	cmp    $0x1,%ax
   1e264:	83 d0 00             	adc    $0x0,%eax
   1e267:	0f b7 f0             	movzwl %ax,%esi
   1e26a:	e9 52 03 00 00       	jmp    1e5c1 <<terminal::sink::ByteSink>::feed+0xb41>
   1e26f:	48 85 c0             	test   %rax,%rax
   1e272:	0f 84 bf 02 00 00    	je     1e537 <<terminal::sink::ByteSink>::feed+0xab7>
   1e278:	8b 44 24 10          	mov    0x10(%rsp),%eax
   1e27c:	66 83 f8 01          	cmp    $0x1,%ax
   1e280:	83 d0 00             	adc    $0x0,%eax
   1e283:	0f b7 f0             	movzwl %ax,%esi
   1e286:	e9 b1 02 00 00       	jmp    1e53c <<terminal::sink::ByteSink>::feed+0xabc>
   1e28b:	48 83 f8 02          	cmp    $0x2,%rax
   1e28f:	0f 83 7a f8 ff ff    	jae    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e295:	48 85 c0             	test   %rax,%rax
   1e298:	0f 84 31 03 00 00    	je     1e5cf <<terminal::sink::ByteSink>::feed+0xb4f>
   1e29e:	8b 44 24 10          	mov    0x10(%rsp),%eax
   1e2a2:	66 83 f8 01          	cmp    $0x1,%ax
   1e2a6:	83 d0 00             	adc    $0x0,%eax
   1e2a9:	0f b7 f0             	movzwl %ax,%esi
   1e2ac:	e9 23 03 00 00       	jmp    1e5d4 <<terminal::sink::ByteSink>::feed+0xb54>
   1e2b1:	48 85 c0             	test   %rax,%rax
   1e2b4:	0f 84 90 02 00 00    	je     1e54a <<terminal::sink::ByteSink>::feed+0xaca>
   1e2ba:	8b 44 24 10          	mov    0x10(%rsp),%eax
   1e2be:	66 83 f8 01          	cmp    $0x1,%ax
   1e2c2:	83 d0 00             	adc    $0x0,%eax
   1e2c5:	0f b7 f0             	movzwl %ax,%esi
   1e2c8:	e9 82 02 00 00       	jmp    1e54f <<terminal::sink::ByteSink>::feed+0xacf>
   1e2cd:	48 85 c0             	test   %rax,%rax
   1e2d0:	0f 84 87 02 00 00    	je     1e55d <<terminal::sink::ByteSink>::feed+0xadd>
   1e2d6:	8b 44 24 10          	mov    0x10(%rsp),%eax
   1e2da:	66 83 f8 01          	cmp    $0x1,%ax
   1e2de:	83 d0 00             	adc    $0x0,%eax
   1e2e1:	0f b7 f0             	movzwl %ax,%esi
   1e2e4:	e9 79 02 00 00       	jmp    1e562 <<terminal::sink::ByteSink>::feed+0xae2>
   1e2e9:	48 85 c0             	test   %rax,%rax
   1e2ec:	0f 84 7e 02 00 00    	je     1e570 <<terminal::sink::ByteSink>::feed+0xaf0>
   1e2f2:	8b 44 24 10          	mov    0x10(%rsp),%eax
   1e2f6:	66 83 f8 01          	cmp    $0x1,%ax
   1e2fa:	83 d0 00             	adc    $0x0,%eax
   1e2fd:	0f b7 f0             	movzwl %ax,%esi
   1e300:	e9 70 02 00 00       	jmp    1e575 <<terminal::sink::ByteSink>::feed+0xaf5>
   1e305:	48 85 c0             	test   %rax,%rax
   1e308:	0f 84 75 02 00 00    	je     1e583 <<terminal::sink::ByteSink>::feed+0xb03>
   1e30e:	8b 44 24 10          	mov    0x10(%rsp),%eax
   1e312:	66 83 f8 01          	cmp    $0x1,%ax
   1e316:	83 d0 00             	adc    $0x0,%eax
   1e319:	0f b7 f0             	movzwl %ax,%esi
   1e31c:	e9 67 02 00 00       	jmp    1e588 <<terminal::sink::ByteSink>::feed+0xb08>
   1e321:	48 85 c0             	test   %rax,%rax
   1e324:	0f 84 6c 02 00 00    	je     1e596 <<terminal::sink::ByteSink>::feed+0xb16>
   1e32a:	8b 44 24 10          	mov    0x10(%rsp),%eax
   1e32e:	66 83 f8 01          	cmp    $0x1,%ax
   1e332:	83 d0 00             	adc    $0x0,%eax
   1e335:	0f b7 f0             	movzwl %ax,%esi
   1e338:	e9 5e 02 00 00       	jmp    1e59b <<terminal::sink::ByteSink>::feed+0xb1b>
   1e33d:	48 83 f8 03          	cmp    $0x3,%rax
   1e341:	0f 83 c8 f7 ff ff    	jae    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e347:	8b 4c 24 10          	mov    0x10(%rsp),%ecx
   1e34b:	48 83 f8 02          	cmp    $0x2,%rax
   1e34f:	0f b7 54 24 12       	movzwl 0x12(%rsp),%edx
   1e354:	be 00 00 00 00       	mov    $0x0,%esi
   1e359:	48 0f 45 d6          	cmovne %rsi,%rdx
   1e35d:	66 83 f9 01          	cmp    $0x1,%cx
   1e361:	83 d1 00             	adc    $0x0,%ecx
   1e364:	48 85 c0             	test   %rax,%rax
   1e367:	0f b7 c1             	movzwl %cx,%eax
   1e36a:	b9 01 00 00 00       	mov    $0x1,%ecx
   1e36f:	48 0f 44 c1          	cmove  %rcx,%rax
   1e373:	48 8b 0c 24          	mov    (%rsp),%rcx
   1e377:	48 8b b1 98 00 00 00 	mov    0x98(%rcx),%rsi
   1e37e:	48 39 d6             	cmp    %rdx,%rsi
   1e381:	48 89 d1             	mov    %rdx,%rcx
   1e384:	48 0f 42 ce          	cmovb  %rsi,%rcx
   1e388:	48 85 d2             	test   %rdx,%rdx
   1e38b:	48 0f 44 ce          	cmove  %rsi,%rcx
   1e38f:	48 39 c8             	cmp    %rcx,%rax
   1e392:	0f 83 77 f7 ff ff    	jae    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e398:	48 ff c8             	dec    %rax
   1e39b:	48 8b 14 24          	mov    (%rsp),%rdx
   1e39f:	48 89 82 b0 00 00 00 	mov    %rax,0xb0(%rdx)
   1e3a6:	48 ff c9             	dec    %rcx
   1e3a9:	48 89 8a b8 00 00 00 	mov    %rcx,0xb8(%rdx)
   1e3b0:	c6 82 d2 00 00 00 00 	movb   $0x0,0xd2(%rdx)
   1e3b7:	48 8b 44 24 60       	mov    0x60(%rsp),%rax
   1e3bc:	48 c7 40 08 00 00 00 	movq   $0x0,0x8(%rax)
   1e3c3:	00 
   1e3c4:	48 c7 00 00 00 00 00 	movq   $0x0,(%rax)
   1e3cb:	e9 3f f7 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e3d0:	48 85 c0             	test   %rax,%rax
   1e3d3:	0f 85 36 f7 ff ff    	jne    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e3d9:	48 8b 34 24          	mov    (%rsp),%rsi
   1e3dd:	0f b6 86 d3 00 00 00 	movzbl 0xd3(%rsi),%eax
   1e3e4:	48 8b 8e a0 00 00 00 	mov    0xa0(%rsi),%rcx
   1e3eb:	48 8b 96 a8 00 00 00 	mov    0xa8(%rsi),%rdx
   1e3f2:	0f b6 b6 d2 00 00 00 	movzbl 0xd2(%rsi),%esi
   1e3f9:	c1 e0 05             	shl    $0x5,%eax
   1e3fc:	48 8b bc 24 b8 00 00 	mov    0xb8(%rsp),%rdi
   1e403:	00 
   1e404:	48 89 0c 07          	mov    %rcx,(%rdi,%rax,1)
   1e408:	48 89 54 07 08       	mov    %rdx,0x8(%rdi,%rax,1)
   1e40d:	48 8b 54 24 68       	mov    0x68(%rsp),%rdx
   1e412:	48 8b 0a             	mov    (%rdx),%rcx
   1e415:	48 89 4c 07 10       	mov    %rcx,0x10(%rdi,%rax,1)
   1e41a:	0f b7 4a 08          	movzwl 0x8(%rdx),%ecx
   1e41e:	66 89 4c 07 18       	mov    %cx,0x18(%rdi,%rax,1)
   1e423:	40 88 74 07 1a       	mov    %sil,0x1a(%rdi,%rax,1)
   1e428:	e9 e2 f6 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e42d:	48 85 c0             	test   %rax,%rax
   1e430:	0f 84 d8 00 00 00    	je     1e50e <<terminal::sink::ByteSink>::feed+0xa8e>
   1e436:	0f b7 74 24 10       	movzwl 0x10(%rsp),%esi
   1e43b:	e9 d0 00 00 00       	jmp    1e510 <<terminal::sink::ByteSink>::feed+0xa90>
   1e440:	48 85 c0             	test   %rax,%rax
   1e443:	0f 84 60 01 00 00    	je     1e5a9 <<terminal::sink::ByteSink>::feed+0xb29>
   1e449:	8b 44 24 10          	mov    0x10(%rsp),%eax
   1e44d:	66 83 f8 01          	cmp    $0x1,%ax
   1e451:	83 d0 00             	adc    $0x0,%eax
   1e454:	0f b7 f0             	movzwl %ax,%esi
   1e457:	e9 52 01 00 00       	jmp    1e5ae <<terminal::sink::ByteSink>::feed+0xb2e>
   1e45c:	48 85 c0             	test   %rax,%rax
   1e45f:	0f 85 aa f6 ff ff    	jne    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e465:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e469:	e8 22 09 00 00       	call   1ed90 <<terminal::grid::TerminalGrid>::restore_cursor>
   1e46e:	e9 9c f6 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e473:	48 85 c0             	test   %rax,%rax
   1e476:	0f 84 a2 00 00 00    	je     1e51e <<terminal::sink::ByteSink>::feed+0xa9e>
   1e47c:	0f b7 74 24 10       	movzwl 0x10(%rsp),%esi
   1e481:	e9 9a 00 00 00       	jmp    1e520 <<terminal::sink::ByteSink>::feed+0xaa0>
   1e486:	66 ba 01 00          	mov    $0x1,%dx
   1e48a:	4c 8b 04 24          	mov    (%rsp),%r8
   1e48e:	0f b7 c2             	movzwl %dx,%eax
   1e491:	49 8b 90 90 00 00 00 	mov    0x90(%r8),%rdx
   1e498:	49 8b b0 98 00 00 00 	mov    0x98(%r8),%rsi
   1e49f:	48 39 c6             	cmp    %rax,%rsi
   1e4a2:	48 0f 42 c6          	cmovb  %rsi,%rax
   1e4a6:	48 ff c8             	dec    %rax
   1e4a9:	49 89 80 a8 00 00 00 	mov    %rax,0xa8(%r8)
   1e4b0:	48 39 ca             	cmp    %rcx,%rdx
   1e4b3:	48 0f 42 ca          	cmovb  %rdx,%rcx
   1e4b7:	48 89 cf             	mov    %rcx,%rdi
   1e4ba:	48 ff cf             	dec    %rdi
   1e4bd:	49 89 b8 a0 00 00 00 	mov    %rdi,0xa0(%r8)
   1e4c4:	41 c6 80 d2 00 00 00 	movb   $0x0,0xd2(%r8)
   1e4cb:	00 
   1e4cc:	0f 84 3d f6 ff ff    	je     1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e4d2:	48 0f af c2          	imul   %rdx,%rax
   1e4d6:	48 01 c7             	add    %rax,%rdi
   1e4d9:	48 8b 04 24          	mov    (%rsp),%rax
   1e4dd:	48 8b 70 10          	mov    0x10(%rax),%rsi
   1e4e1:	48 39 f7             	cmp    %rsi,%rdi
   1e4e4:	0f 83 1b 01 00 00    	jae    1e605 <<terminal::sink::ByteSink>::feed+0xb85>
   1e4ea:	48 8b 40 08          	mov    0x8(%rax),%rax
   1e4ee:	48 8d 14 bf          	lea    (%rdi,%rdi,4),%rdx
   1e4f2:	80 7c 90 12 02       	cmpb   $0x2,0x12(%rax,%rdx,4)
   1e4f7:	0f 85 12 f6 ff ff    	jne    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e4fd:	48 83 c1 fe          	add    $0xfffffffffffffffe,%rcx
   1e501:	48 8b 44 24 60       	mov    0x60(%rsp),%rax
   1e506:	48 89 08             	mov    %rcx,(%rax)
   1e509:	e9 01 f6 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e50e:	31 f6                	xor    %esi,%esi
   1e510:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e514:	e8 27 5b 00 00       	call   24040 <<terminal::grid::TerminalGrid>::erase_display>
   1e519:	e9 f1 f5 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e51e:	31 f6                	xor    %esi,%esi
   1e520:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e524:	e8 67 55 00 00       	call   23a90 <<terminal::grid::TerminalGrid>::erase_line>
   1e529:	e9 e1 f5 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e52e:	66 b8 ff ff          	mov    $0xffff,%ax
   1e532:	e9 96 f8 ff ff       	jmp    1ddcd <<terminal::sink::ByteSink>::feed+0x34d>
   1e537:	be 01 00 00 00       	mov    $0x1,%esi
   1e53c:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e540:	e8 db 72 00 00       	call   25820 <<terminal::grid::TerminalGrid>::next_line>
   1e545:	e9 c5 f5 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e54a:	be 01 00 00 00       	mov    $0x1,%esi
   1e54f:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e553:	e8 48 06 00 00       	call   1eba0 <<terminal::grid::TerminalGrid>::cursor_down>
   1e558:	e9 b2 f5 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e55d:	be 01 00 00 00       	mov    $0x1,%esi
   1e562:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e566:	e8 05 5c 00 00       	call   24170 <<terminal::grid::TerminalGrid>::previous_line>
   1e56b:	e9 9f f5 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e570:	be 01 00 00 00       	mov    $0x1,%esi
   1e575:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e579:	e8 32 07 00 00       	call   1ecb0 <<terminal::grid::TerminalGrid>::cursor_back>
   1e57e:	e9 8c f5 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e583:	be 01 00 00 00       	mov    $0x1,%esi
   1e588:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e58c:	e8 7f 05 00 00       	call   1eb10 <<terminal::grid::TerminalGrid>::cursor_up>
   1e591:	e9 79 f5 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e596:	be 01 00 00 00       	mov    $0x1,%esi
   1e59b:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e59f:	e8 8c 06 00 00       	call   1ec30 <<terminal::grid::TerminalGrid>::cursor_forward>
   1e5a4:	e9 66 f5 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e5a9:	be 01 00 00 00       	mov    $0x1,%esi
   1e5ae:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e5b2:	e8 69 07 00 00       	call   1ed20 <<terminal::grid::TerminalGrid>::set_column>
   1e5b7:	e9 53 f5 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e5bc:	be 01 00 00 00       	mov    $0x1,%esi
   1e5c1:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e5c5:	e8 56 58 00 00       	call   23e20 <<terminal::grid::TerminalGrid>::scroll_down>
   1e5ca:	e9 40 f5 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e5cf:	be 01 00 00 00       	mov    $0x1,%esi
   1e5d4:	48 8b 3c 24          	mov    (%rsp),%rdi
   1e5d8:	e8 73 64 00 00       	call   24a50 <<terminal::grid::TerminalGrid>::scroll_up>
   1e5dd:	e9 2d f5 ff ff       	jmp    1db0f <<terminal::sink::ByteSink>::feed+0x8f>
   1e5e2:	48 81 c4 d8 00 00 00 	add    $0xd8,%rsp
   1e5e9:	5b                   	pop    %rbx
   1e5ea:	41 5c                	pop    %r12
   1e5ec:	41 5d                	pop    %r13
   1e5ee:	41 5e                	pop    %r14
   1e5f0:	41 5f                	pop    %r15
   1e5f2:	5d                   	pop    %rbp
   1e5f3:	c3                   	ret
   1e5f4:	48 8d 15 05 e2 0b 00 	lea    0xbe205(%rip),%rdx        # dc800 <ostd::startup::_KEEP_START+0x128>
   1e5fb:	be 04 00 00 00       	mov    $0x4,%esi
   1e600:	e8 b4 bc fe ff       	call   a2b9 <core::panicking::panic_bounds_check>
   1e605:	48 8d 15 3c e2 0b 00 	lea    0xbe23c(%rip),%rdx        # dc848 <ostd::startup::_KEEP_START+0x170>
   1e60c:	e8 a8 bc fe ff       	call   a2b9 <core::panicking::panic_bounds_check>
   1e611:	cc                   	int3
   1e612:	cc                   	int3
   1e613:	cc                   	int3
   1e614:	cc                   	int3
   1e615:	cc                   	int3
   1e616:	cc                   	int3
   1e617:	cc                   	int3
   1e618:	cc                   	int3
   1e619:	cc                   	int3
   1e61a:	cc                   	int3
   1e61b:	cc                   	int3
   1e61c:	cc                   	int3
   1e61d:	cc                   	int3
   1e61e:	cc                   	int3
   1e61f:	cc                   	int3
