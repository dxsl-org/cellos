
/tmp/cellbox-s10-3d-resume/cand/desktop/files/cellbox:     file format elf64-x86-64


Disassembly of section .text:

0000000000017be0 <cellbox::executor::terminal_write>:
   17be0:	55                   	push   %rbp
   17be1:	41 57                	push   %r15
   17be3:	41 56                	push   %r14
   17be5:	41 55                	push   %r13
   17be7:	41 54                	push   %r12
   17be9:	53                   	push   %rbx
   17bea:	48 83 ec 28          	sub    $0x28,%rsp
   17bee:	48 89 f3             	mov    %rsi,%rbx
   17bf1:	48 89 7c 24 10       	mov    %rdi,0x10(%rsp)
   17bf6:	b1 01                	mov    $0x1,%cl
   17bf8:	0f 1f 84 00 00 00 00 	nopl   0x0(%rax,%rax,1)
   17bff:	00 
   17c00:	31 c0                	xor    %eax,%eax
   17c02:	f0 0f b0 0d fe 03 29 	lock cmpxchg %cl,0x2903fe(%rip)        # 2a8008 <cellbox::executor::FRONTEND_OUTPUT>
   17c09:	00 
   17c0a:	74 13                	je     17c1f <cellbox::executor::terminal_write+0x3f>
   17c0c:	0f 1f 40 00          	nopl   0x0(%rax)
   17c10:	0f b6 05 f1 03 29 00 	movzbl 0x2903f1(%rip),%eax        # 2a8008 <cellbox::executor::FRONTEND_OUTPUT>
   17c17:	84 c0                	test   %al,%al
   17c19:	74 e5                	je     17c00 <cellbox::executor::terminal_write+0x20>
   17c1b:	f3 90                	pause
   17c1d:	eb f1                	jmp    17c10 <cellbox::executor::terminal_write+0x30>
   17c1f:	48 83 3d e9 03 29 00 	cmpq   $0x0,0x2903e9(%rip)        # 2a8010 <cellbox::executor::FRONTEND_OUTPUT+0x8>
   17c26:	00 
   17c27:	c6 05 da 03 29 00 00 	movb   $0x0,0x2903da(%rip)        # 2a8008 <cellbox::executor::FRONTEND_OUTPUT>
   17c2e:	74 0c                	je     17c3c <cellbox::executor::terminal_write+0x5c>
   17c30:	0f b6 05 c9 03 29 00 	movzbl 0x2903c9(%rip),%eax        # 2a8000 <cellbox::executor::STREAMS_MERGED>
   17c37:	84 c0                	test   %al,%al
   17c39:	0f 94 c1             	sete   %cl
   17c3c:	66 b8 ff ff          	mov    $0xffff,%ax
   17c40:	48 85 db             	test   %rbx,%rbx
   17c43:	0f 84 f8 00 00 00    	je     17d41 <cellbox::executor::terminal_write+0x161>
   17c49:	48 8d 35 50 86 26 00 	lea    0x268650(%rip),%rsi        # 2802a0 <cellbox::executor::SERIAL_SESSION+0x150>
   17c50:	48 8d 05 29 86 26 00 	lea    0x268629(%rip),%rax        # 280280 <cellbox::executor::SERIAL_SESSION+0x130>
   17c57:	84 c9                	test   %cl,%cl
   17c59:	48 0f 44 f0          	cmove  %rax,%rsi
   17c5d:	84 d2                	test   %dl,%dl
   17c5f:	48 0f 44 f0          	cmove  %rax,%rsi
   17c63:	48 89 74 24 08       	mov    %rsi,0x8(%rsp)
   17c68:	40 b5 01             	mov    $0x1,%bpl
   17c6b:	45 31 e4             	xor    %r12d,%r12d
   17c6e:	66 90                	xchg   %ax,%ax
   17c70:	31 c0                	xor    %eax,%eax
   17c72:	f0 40 0f b0 2d d5 84 	lock cmpxchg %bpl,0x2684d5(%rip)        # 280150 <cellbox::executor::SERIAL_SESSION>
   17c79:	26 00 
   17c7b:	75 65                	jne    17ce2 <cellbox::executor::terminal_write+0x102>
   17c7d:	83 3d d4 84 26 00 ff 	cmpl   $0xffffffff,0x2684d4(%rip)        # 280158 <cellbox::executor::SERIAL_SESSION+0x8>
   17c84:	74 69                	je     17cef <cellbox::executor::terminal_write+0x10f>
   17c86:	49 89 dd             	mov    %rbx,%r13
   17c89:	4d 29 e5             	sub    %r12,%r13
   17c8c:	48 8b 44 24 10       	mov    0x10(%rsp),%rax
   17c91:	4a 8d 14 20          	lea    (%rax,%r12,1),%rdx
   17c95:	48 8d 7c 24 18       	lea    0x18(%rsp),%rdi
   17c9a:	48 8b 74 24 08       	mov    0x8(%rsp),%rsi
   17c9f:	4c 89 e9             	mov    %r13,%rcx
   17ca2:	45 31 c0             	xor    %r8d,%r8d
   17ca5:	e8 06 c6 18 00       	call   1a42b0 <<terminal_sdk::stream::Endpoint>::write>
   17caa:	44 0f b6 7c 24 18    	movzbl 0x18(%rsp),%r15d
   17cb0:	44 0f b7 74 24 1a    	movzwl 0x1a(%rsp),%r14d
   17cb6:	66 41 83 fe 0e       	cmp    $0xe,%r14w
   17cbb:	0f 95 c0             	setne  %al
   17cbe:	41 84 c7             	test   %al,%r15b
   17cc1:	75 31                	jne    17cf4 <cellbox::executor::terminal_write+0x114>
   17cc3:	31 ff                	xor    %edi,%edi
   17cc5:	e8 16 a7 ff ff       	call   123e0 <cellbox::executor::relay_serial>
   17cca:	a8 01                	test   $0x1,%al
   17ccc:	74 3f                	je     17d0d <cellbox::executor::terminal_write+0x12d>
   17cce:	c1 e8 10             	shr    $0x10,%eax
   17cd1:	41 89 c6             	mov    %eax,%r14d
   17cd4:	eb 1e                	jmp    17cf4 <cellbox::executor::terminal_write+0x114>
   17cd6:	66 2e 0f 1f 84 00 00 	cs nopw 0x0(%rax,%rax,1)
   17cdd:	00 00 00 
   17ce0:	f3 90                	pause
   17ce2:	0f b6 05 67 84 26 00 	movzbl 0x268467(%rip),%eax        # 280150 <cellbox::executor::SERIAL_SESSION>
   17ce9:	84 c0                	test   %al,%al
   17ceb:	75 f3                	jne    17ce0 <cellbox::executor::terminal_write+0x100>
   17ced:	eb 81                	jmp    17c70 <cellbox::executor::terminal_write+0x90>
   17cef:	66 41 be 0a 00       	mov    $0xa,%r14w
   17cf4:	c6 05 55 84 26 00 00 	movb   $0x0,0x268455(%rip)        # 280150 <cellbox::executor::SERIAL_SESSION>
   17cfb:	66 41 83 fe 0e       	cmp    $0xe,%r14w
   17d00:	75 3c                	jne    17d3e <cellbox::executor::terminal_write+0x15e>
   17d02:	49 39 dc             	cmp    %rbx,%r12
   17d05:	0f 82 65 ff ff ff    	jb     17c70 <cellbox::executor::terminal_write+0x90>
   17d0b:	eb 26                	jmp    17d33 <cellbox::executor::terminal_write+0x153>
   17d0d:	c6 05 3c 84 26 00 00 	movb   $0x0,0x26843c(%rip)        # 280150 <cellbox::executor::SERIAL_SESSION>
   17d14:	45 84 ff             	test   %r15b,%r15b
   17d17:	75 e2                	jne    17cfb <cellbox::executor::terminal_write+0x11b>
   17d19:	48 8b 44 24 20       	mov    0x20(%rsp),%rax
   17d1e:	48 8d 48 ff          	lea    -0x1(%rax),%rcx
   17d22:	4c 39 e9             	cmp    %r13,%rcx
   17d25:	73 12                	jae    17d39 <cellbox::executor::terminal_write+0x159>
   17d27:	49 01 c4             	add    %rax,%r12
   17d2a:	49 39 dc             	cmp    %rbx,%r12
   17d2d:	0f 82 3d ff ff ff    	jb     17c70 <cellbox::executor::terminal_write+0x90>
   17d33:	66 b8 ff ff          	mov    $0xffff,%ax
   17d37:	eb 08                	jmp    17d41 <cellbox::executor::terminal_write+0x161>
   17d39:	66 41 be 03 00       	mov    $0x3,%r14w
   17d3e:	44 89 f0             	mov    %r14d,%eax
   17d41:	48 83 c4 28          	add    $0x28,%rsp
   17d45:	5b                   	pop    %rbx
   17d46:	41 5c                	pop    %r12
   17d48:	41 5d                	pop    %r13
   17d4a:	41 5e                	pop    %r14
   17d4c:	41 5f                	pop    %r15
   17d4e:	5d                   	pop    %rbp
   17d4f:	c3                   	ret
