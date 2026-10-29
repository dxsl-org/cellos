
/tmp/cellbox-s10-3d-resume/cand/desktop/files/terminal:     file format elf64-x86-64


Disassembly of section .text:

0000000000025c10 <<terminal_sdk::host::FrontendHost>::poll_child>:
   25c10:	41 57                	push   %r15
   25c12:	41 56                	push   %r14
   25c14:	53                   	push   %rbx
   25c15:	48 83 ec 60          	sub    $0x60,%rsp
   25c19:	ba 01 00 00 00       	mov    $0x1,%edx
   25c1e:	80 bf 4c 01 00 00 00 	cmpb   $0x0,0x14c(%rdi)
   25c25:	74 07                	je     25c2e <<terminal_sdk::host::FrontendHost>::poll_child+0x1e>
   25c27:	b9 00 00 11 00       	mov    $0x110000,%ecx
   25c2c:	eb 20                	jmp    25c4e <<terminal_sdk::host::FrontendHost>::poll_child+0x3e>
   25c2e:	48 8b 77 58          	mov    0x58(%rdi),%rsi
   25c32:	b0 01                	mov    $0x1,%al
   25c34:	31 c9                	xor    %ecx,%ecx
   25c36:	48 83 fe ff          	cmp    $0xffffffffffffffff,%rsi
   25c3a:	74 10                	je     25c4c <<terminal_sdk::host::FrontendHost>::poll_child+0x3c>
   25c3c:	83 bf 94 00 00 00 ff 	cmpl   $0xffffffff,0x94(%rdi)
   25c43:	74 1d                	je     25c62 <<terminal_sdk::host::FrontendHost>::poll_child+0x52>
   25c45:	0f b6 87 90 00 00 00 	movzbl 0x90(%rdi),%eax
   25c4c:	31 d2                	xor    %edx,%edx
   25c4e:	09 d1                	or     %edx,%ecx
   25c50:	0f b6 c0             	movzbl %al,%eax
   25c53:	c1 e0 08             	shl    $0x8,%eax
   25c56:	09 c8                	or     %ecx,%eax
   25c58:	48 83 c4 60          	add    $0x60,%rsp
   25c5c:	5b                   	pop    %rbx
   25c5d:	41 5e                	pop    %r14
   25c5f:	41 5f                	pop    %r15
   25c61:	c3                   	ret
   25c62:	ba 00 00 00 00       	mov    $0x0,%edx
   25c67:	83 fe 01             	cmp    $0x1,%esi
   25c6a:	75 e2                	jne    25c4e <<terminal_sdk::host::FrontendHost>::poll_child+0x3e>
   25c6c:	48 c7 44 24 38 00 00 	movq   $0x0,0x38(%rsp)
   25c73:	00 00 
   25c75:	48 c7 44 24 30 00 00 	movq   $0x0,0x30(%rsp)
   25c7c:	00 00 
   25c7e:	48 c7 44 24 28 00 00 	movq   $0x0,0x28(%rsp)
   25c85:	00 00 
   25c87:	48 c7 44 24 20 00 00 	movq   $0x0,0x20(%rsp)
   25c8e:	00 00 
   25c90:	48 c7 44 24 18 00 00 	movq   $0x0,0x18(%rsp)
   25c97:	00 00 
   25c99:	48 c7 44 24 10 00 00 	movq   $0x0,0x10(%rsp)
   25ca0:	00 00 
   25ca2:	48 8b 97 80 00 00 00 	mov    0x80(%rdi),%rdx
   25ca9:	48 8b 8f 88 00 00 00 	mov    0x88(%rdi),%rcx
   25cb0:	48 8b 47 60          	mov    0x60(%rdi),%rax
   25cb4:	48 8b 77 68          	mov    0x68(%rdi),%rsi
   25cb8:	4c 8b 47 70          	mov    0x70(%rdi),%r8
   25cbc:	49 89 ff             	mov    %rdi,%r15
   25cbf:	48 8b 7f 78          	mov    0x78(%rdi),%rdi
   25cc3:	48 89 44 24 40       	mov    %rax,0x40(%rsp)
   25cc8:	48 89 74 24 48       	mov    %rsi,0x48(%rsp)
   25ccd:	4c 89 44 24 50       	mov    %r8,0x50(%rsp)
   25cd2:	48 89 7c 24 58       	mov    %rdi,0x58(%rsp)
   25cd7:	48 83 ec 08          	sub    $0x8,%rsp
   25cdb:	48 8d 44 24 18       	lea    0x18(%rsp),%rax
   25ce0:	4c 8d 74 24 08       	lea    0x8(%rsp),%r14
   25ce5:	48 8d 5c 24 48       	lea    0x48(%rsp),%rbx
   25cea:	4c 89 f7             	mov    %r14,%rdi
   25ced:	be 1e 00 00 00       	mov    $0x1e,%esi
   25cf2:	45 31 c0             	xor    %r8d,%r8d
   25cf5:	49 89 d9             	mov    %rbx,%r9
   25cf8:	6a 30                	push   $0x30
   25cfa:	50                   	push   %rax
   25cfb:	6a 20                	push   $0x20
   25cfd:	e8 ce 75 fe ff       	call   d2d0 <ostd::session::kernel_call>
   25d02:	48 83 c4 20          	add    $0x20,%rsp
   25d06:	0f b7 44 24 0e       	movzwl 0xe(%rsp),%eax
   25d0b:	66 83 f8 ff          	cmp    $0xffff,%ax
   25d0f:	74 70                	je     25d81 <<terminal_sdk::host::FrontendHost>::poll_child+0x171>
   25d11:	66 85 c0             	test   %ax,%ax
   25d14:	75 6f                	jne    25d85 <<terminal_sdk::host::FrontendHost>::poll_child+0x175>
   25d16:	8b 74 24 08          	mov    0x8(%rsp),%esi
   25d1a:	48 8d 7e e8          	lea    -0x18(%rsi),%rdi
   25d1e:	48 83 ff 19          	cmp    $0x19,%rdi
   25d22:	0f 83 9e 00 00 00    	jae    25dc6 <<terminal_sdk::host::FrontendHost>::poll_child+0x1b6>
   25d28:	ba ff ff ff ff       	mov    $0xffffffff,%edx
   25d2d:	66 b8 03 00          	mov    $0x3,%ax
   25d31:	48 83 ff 04          	cmp    $0x4,%rdi
   25d35:	4c 89 f9             	mov    %r15,%rcx
   25d38:	72 19                	jb     25d53 <<terminal_sdk::host::FrontendHost>::poll_child+0x143>
   25d3a:	48 83 ff 08          	cmp    $0x8,%rdi
   25d3e:	72 13                	jb     25d53 <<terminal_sdk::host::FrontendHost>::poll_child+0x143>
   25d40:	48 83 ff 0c          	cmp    $0xc,%rdi
   25d44:	72 0d                	jb     25d53 <<terminal_sdk::host::FrontendHost>::poll_child+0x143>
   25d46:	48 83 ff 18          	cmp    $0x18,%rdi
   25d4a:	75 07                	jne    25d53 <<terminal_sdk::host::FrontendHost>::poll_child+0x143>
   25d4c:	83 7c 24 34 00       	cmpl   $0x0,0x34(%rsp)
   25d51:	74 41                	je     25d94 <<terminal_sdk::host::FrontendHost>::poll_child+0x184>
   25d53:	89 13                	mov    %edx,(%rbx)
   25d55:	8b 54 24 40          	mov    0x40(%rsp),%edx
   25d59:	83 fa ff             	cmp    $0xffffffff,%edx
   25d5c:	74 27                	je     25d85 <<terminal_sdk::host::FrontendHost>::poll_child+0x175>
   25d5e:	a8 01                	test   $0x1,%al
   25d60:	74 16                	je     25d78 <<terminal_sdk::host::FrontendHost>::poll_child+0x168>
   25d62:	8b 34 24             	mov    (%rsp),%esi
   25d65:	66 89 81 90 00 00 00 	mov    %ax,0x90(%rcx)
   25d6c:	89 91 94 00 00 00    	mov    %edx,0x94(%rcx)
   25d72:	89 b1 98 00 00 00    	mov    %esi,0x98(%rcx)
   25d78:	24 01                	and    $0x1,%al
   25d7a:	31 c9                	xor    %ecx,%ecx
   25d7c:	e9 cb fe ff ff       	jmp    25c4c <<terminal_sdk::host::FrontendHost>::poll_child+0x3c>
   25d81:	0f b7 04 24          	movzwl (%rsp),%eax
   25d85:	c1 e0 10             	shl    $0x10,%eax
   25d88:	89 c1                	mov    %eax,%ecx
   25d8a:	ba 01 00 00 00       	mov    $0x1,%edx
   25d8f:	e9 ba fe ff ff       	jmp    25c4e <<terminal_sdk::host::FrontendHost>::poll_child+0x3e>
   25d94:	8b 74 24 28          	mov    0x28(%rsp),%esi
   25d98:	8d 7e ff             	lea    -0x1(%rsi),%edi
   25d9b:	83 ff 02             	cmp    $0x2,%edi
   25d9e:	73 06                	jae    25da6 <<terminal_sdk::host::FrontendHost>::poll_child+0x196>
   25da0:	31 c0                	xor    %eax,%eax
   25da2:	31 d2                	xor    %edx,%edx
   25da4:	eb ad                	jmp    25d53 <<terminal_sdk::host::FrontendHost>::poll_child+0x143>
   25da6:	83 fe 03             	cmp    $0x3,%esi
   25da9:	75 a8                	jne    25d53 <<terminal_sdk::host::FrontendHost>::poll_child+0x143>
   25dab:	8b 54 24 30          	mov    0x30(%rsp),%edx
   25daf:	31 c0                	xor    %eax,%eax
   25db1:	83 7c 24 2c 00       	cmpl   $0x0,0x2c(%rsp)
   25db6:	0f 94 c0             	sete   %al
   25db9:	89 44 24 40          	mov    %eax,0x40(%rsp)
   25dbd:	66 b8 01 00          	mov    $0x1,%ax
   25dc1:	4c 89 f3             	mov    %r14,%rbx
   25dc4:	eb 8d                	jmp    25d53 <<terminal_sdk::host::FrontendHost>::poll_child+0x143>
   25dc6:	48 8d 0d 43 6c 0b 00 	lea    0xb6c43(%rip),%rcx        # dca10 <ostd::startup::_KEEP_START+0x338>
   25dcd:	bf 18 00 00 00       	mov    $0x18,%edi
   25dd2:	ba 30 00 00 00       	mov    $0x30,%edx
   25dd7:	e8 e4 2f fe ff       	call   8dc0 <core::slice::index::slice_index_fail>
