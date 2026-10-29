
/tmp/cellbox-s10-3d-resume/cand/desktop.kernel.elf:     file format elf64-x86-64


Disassembly of section .text:

ffffffff8005fdb0 <<cellos_kernel::task::cell_session::State>::is_subtree>:
ffffffff8005fdb0:	41 56                	push   %r14
ffffffff8005fdb2:	53                   	push   %rbx
ffffffff8005fdb3:	50                   	push   %rax
ffffffff8005fdb4:	49 89 d2             	mov    %rdx,%r10
ffffffff8005fdb7:	4d 31 c2             	xor    %r8,%r10
ffffffff8005fdba:	49 89 cb             	mov    %rcx,%r11
ffffffff8005fdbd:	4d 31 cb             	xor    %r9,%r11
ffffffff8005fdc0:	b0 01                	mov    $0x1,%al
ffffffff8005fdc2:	4d 09 d3             	or     %r10,%r11
ffffffff8005fdc5:	0f 84 c6 00 00 00    	je     ffffffff8005fe91 <<cellos_kernel::task::cell_session::State>::is_subtree+0xe1>
ffffffff8005fdcb:	48 85 f6             	test   %rsi,%rsi
ffffffff8005fdce:	0f 84 bb 00 00 00    	je     ffffffff8005fe8f <<cellos_kernel::task::cell_session::State>::is_subtree+0xdf>
ffffffff8005fdd4:	48 83 fe 01          	cmp    $0x1,%rsi
ffffffff8005fdd8:	75 11                	jne    ffffffff8005fdeb <<cellos_kernel::task::cell_session::State>::is_subtree+0x3b>
ffffffff8005fdda:	48 39 17             	cmp    %rdx,(%rdi)
ffffffff8005fddd:	0f 85 8d 00 00 00    	jne    ffffffff8005fe70 <<cellos_kernel::task::cell_session::State>::is_subtree+0xc0>
ffffffff8005fde3:	45 31 d2             	xor    %r10d,%r10d
ffffffff8005fde6:	48 89 f8             	mov    %rdi,%rax
ffffffff8005fde9:	eb 43                	jmp    ffffffff8005fe2e <<cellos_kernel::task::cell_session::State>::is_subtree+0x7e>
ffffffff8005fdeb:	45 31 d2             	xor    %r10d,%r10d
ffffffff8005fdee:	48 89 f0             	mov    %rsi,%rax
ffffffff8005fdf1:	66 66 66 66 66 66 2e 	data16 data16 data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
ffffffff8005fdf8:	0f 1f 84 00 00 00 00 
ffffffff8005fdff:	00 
ffffffff8005fe00:	4d 89 d3             	mov    %r10,%r11
ffffffff8005fe03:	48 89 c3             	mov    %rax,%rbx
ffffffff8005fe06:	48 d1 eb             	shr    $1,%rbx
ffffffff8005fe09:	49 01 da             	add    %rbx,%r10
ffffffff8005fe0c:	4d 6b f2 58          	imul   $0x58,%r10,%r14
ffffffff8005fe10:	4a 39 14 37          	cmp    %rdx,(%rdi,%r14,1)
ffffffff8005fe14:	4d 0f 47 d3          	cmova  %r11,%r10
ffffffff8005fe18:	48 29 d8             	sub    %rbx,%rax
ffffffff8005fe1b:	48 83 f8 01          	cmp    $0x1,%rax
ffffffff8005fe1f:	77 df                	ja     ffffffff8005fe00 <<cellos_kernel::task::cell_session::State>::is_subtree+0x50>
ffffffff8005fe21:	49 6b c2 58          	imul   $0x58,%r10,%rax
ffffffff8005fe25:	48 39 14 07          	cmp    %rdx,(%rdi,%rax,1)
ffffffff8005fe29:	75 45                	jne    ffffffff8005fe70 <<cellos_kernel::task::cell_session::State>::is_subtree+0xc0>
ffffffff8005fe2b:	48 01 f8             	add    %rdi,%rax
ffffffff8005fe2e:	48 39 48 08          	cmp    %rcx,0x8(%rax)
ffffffff8005fe32:	0f 95 c2             	setne  %dl
ffffffff8005fe35:	48 83 fe 01          	cmp    $0x1,%rsi
ffffffff8005fe39:	74 3d                	je     ffffffff8005fe78 <<cellos_kernel::task::cell_session::State>::is_subtree+0xc8>
ffffffff8005fe3b:	31 c9                	xor    %ecx,%ecx
ffffffff8005fe3d:	48 89 f0             	mov    %rsi,%rax
ffffffff8005fe40:	49 89 cb             	mov    %rcx,%r11
ffffffff8005fe43:	48 89 c3             	mov    %rax,%rbx
ffffffff8005fe46:	48 d1 eb             	shr    $1,%rbx
ffffffff8005fe49:	48 01 d9             	add    %rbx,%rcx
ffffffff8005fe4c:	4c 6b f1 58          	imul   $0x58,%rcx,%r14
ffffffff8005fe50:	4e 39 04 37          	cmp    %r8,(%rdi,%r14,1)
ffffffff8005fe54:	49 0f 47 cb          	cmova  %r11,%rcx
ffffffff8005fe58:	48 29 d8             	sub    %rbx,%rax
ffffffff8005fe5b:	48 83 f8 01          	cmp    $0x1,%rax
ffffffff8005fe5f:	77 df                	ja     ffffffff8005fe40 <<cellos_kernel::task::cell_session::State>::is_subtree+0x90>
ffffffff8005fe61:	48 6b c1 58          	imul   $0x58,%rcx,%rax
ffffffff8005fe65:	4c 39 04 07          	cmp    %r8,(%rdi,%rax,1)
ffffffff8005fe69:	75 24                	jne    ffffffff8005fe8f <<cellos_kernel::task::cell_session::State>::is_subtree+0xdf>
ffffffff8005fe6b:	48 01 f8             	add    %rdi,%rax
ffffffff8005fe6e:	eb 14                	jmp    ffffffff8005fe84 <<cellos_kernel::task::cell_session::State>::is_subtree+0xd4>
ffffffff8005fe70:	b2 01                	mov    $0x1,%dl
ffffffff8005fe72:	48 83 fe 01          	cmp    $0x1,%rsi
ffffffff8005fe76:	75 c3                	jne    ffffffff8005fe3b <<cellos_kernel::task::cell_session::State>::is_subtree+0x8b>
ffffffff8005fe78:	31 c0                	xor    %eax,%eax
ffffffff8005fe7a:	4c 39 07             	cmp    %r8,(%rdi)
ffffffff8005fe7d:	75 12                	jne    ffffffff8005fe91 <<cellos_kernel::task::cell_session::State>::is_subtree+0xe1>
ffffffff8005fe7f:	31 c9                	xor    %ecx,%ecx
ffffffff8005fe81:	48 89 f8             	mov    %rdi,%rax
ffffffff8005fe84:	4c 39 48 08          	cmp    %r9,0x8(%rax)
ffffffff8005fe88:	0f 95 c0             	setne  %al
ffffffff8005fe8b:	08 c2                	or     %al,%dl
ffffffff8005fe8d:	74 0a                	je     ffffffff8005fe99 <<cellos_kernel::task::cell_session::State>::is_subtree+0xe9>
ffffffff8005fe8f:	31 c0                	xor    %eax,%eax
ffffffff8005fe91:	48 83 c4 08          	add    $0x8,%rsp
ffffffff8005fe95:	5b                   	pop    %rbx
ffffffff8005fe96:	41 5e                	pop    %r14
ffffffff8005fe98:	c3                   	ret
ffffffff8005fe99:	48 6b c1 58          	imul   $0x58,%rcx,%rax
ffffffff8005fe9d:	8b 54 07 4c          	mov    0x4c(%rdi,%rax,1),%edx
ffffffff8005fea1:	eb 15                	jmp    ffffffff8005feb8 <<cellos_kernel::task::cell_session::State>::is_subtree+0x108>
ffffffff8005fea3:	66 66 66 66 2e 0f 1f 	data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
ffffffff8005feaa:	84 00 00 00 00 00 
ffffffff8005feb0:	31 c0                	xor    %eax,%eax
ffffffff8005feb2:	4d 39 41 08          	cmp    %r8,0x8(%r9)
ffffffff8005feb6:	75 d9                	jne    ffffffff8005fe91 <<cellos_kernel::task::cell_session::State>::is_subtree+0xe1>
ffffffff8005feb8:	49 39 f2             	cmp    %rsi,%r10
ffffffff8005febb:	0f 83 81 00 00 00    	jae    ffffffff8005ff42 <<cellos_kernel::task::cell_session::State>::is_subtree+0x192>
ffffffff8005fec1:	4d 6b c2 58          	imul   $0x58,%r10,%r8
ffffffff8005fec5:	42 39 54 07 4c       	cmp    %edx,0x4c(%rdi,%r8,1)
ffffffff8005feca:	76 6b                	jbe    ffffffff8005ff37 <<cellos_kernel::task::cell_session::State>::is_subtree+0x187>
ffffffff8005fecc:	49 01 f8             	add    %rdi,%r8
ffffffff8005fecf:	49 8b 40 20          	mov    0x20(%r8),%rax
ffffffff8005fed3:	4d 8b 40 28          	mov    0x28(%r8),%r8
ffffffff8005fed7:	48 83 fe 01          	cmp    $0x1,%rsi
ffffffff8005fedb:	75 13                	jne    ffffffff8005fef0 <<cellos_kernel::task::cell_session::State>::is_subtree+0x140>
ffffffff8005fedd:	45 31 d2             	xor    %r10d,%r10d
ffffffff8005fee0:	48 39 07             	cmp    %rax,(%rdi)
ffffffff8005fee3:	75 aa                	jne    ffffffff8005fe8f <<cellos_kernel::task::cell_session::State>::is_subtree+0xdf>
ffffffff8005fee5:	49 89 f9             	mov    %rdi,%r9
ffffffff8005fee8:	eb c6                	jmp    ffffffff8005feb0 <<cellos_kernel::task::cell_session::State>::is_subtree+0x100>
ffffffff8005feea:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
ffffffff8005fef0:	49 89 f1             	mov    %rsi,%r9
ffffffff8005fef3:	45 31 d2             	xor    %r10d,%r10d
ffffffff8005fef6:	66 2e 0f 1f 84 00 00 	cs nopw 0x0(%rax,%rax,1)
ffffffff8005fefd:	00 00 00 
ffffffff8005ff00:	4d 89 d3             	mov    %r10,%r11
ffffffff8005ff03:	4c 89 cb             	mov    %r9,%rbx
ffffffff8005ff06:	48 d1 eb             	shr    $1,%rbx
ffffffff8005ff09:	49 01 da             	add    %rbx,%r10
ffffffff8005ff0c:	4d 6b f2 58          	imul   $0x58,%r10,%r14
ffffffff8005ff10:	4a 39 04 37          	cmp    %rax,(%rdi,%r14,1)
ffffffff8005ff14:	4d 0f 47 d3          	cmova  %r11,%r10
ffffffff8005ff18:	49 29 d9             	sub    %rbx,%r9
ffffffff8005ff1b:	49 83 f9 01          	cmp    $0x1,%r9
ffffffff8005ff1f:	77 df                	ja     ffffffff8005ff00 <<cellos_kernel::task::cell_session::State>::is_subtree+0x150>
ffffffff8005ff21:	4d 6b ca 58          	imul   $0x58,%r10,%r9
ffffffff8005ff25:	4a 39 04 0f          	cmp    %rax,(%rdi,%r9,1)
ffffffff8005ff29:	0f 85 60 ff ff ff    	jne    ffffffff8005fe8f <<cellos_kernel::task::cell_session::State>::is_subtree+0xdf>
ffffffff8005ff2f:	49 01 f9             	add    %rdi,%r9
ffffffff8005ff32:	e9 79 ff ff ff       	jmp    ffffffff8005feb0 <<cellos_kernel::task::cell_session::State>::is_subtree+0x100>
ffffffff8005ff37:	49 39 ca             	cmp    %rcx,%r10
ffffffff8005ff3a:	0f 94 c0             	sete   %al
ffffffff8005ff3d:	e9 4f ff ff ff       	jmp    ffffffff8005fe91 <<cellos_kernel::task::cell_session::State>::is_subtree+0xe1>
ffffffff8005ff42:	4c 89 d7             	mov    %r10,%rdi
ffffffff8005ff45:	48 c7 c2 a8 26 19 80 	mov    $0xffffffff801926a8,%rdx
ffffffff8005ff4c:	e8 e1 a9 0c 00       	call   ffffffff8012a932 <core::panicking::panic_bounds_check>
