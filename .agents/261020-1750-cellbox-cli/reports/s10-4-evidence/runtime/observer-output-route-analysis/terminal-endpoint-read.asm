
/tmp/cellbox-s10-3d-resume/cand/desktop/files/terminal:     file format elf64-x86-64


Disassembly of section .text:

0000000000026e60 <<terminal_sdk::stream::Endpoint>::read>:
   26e60:	55                   	push   %rbp
   26e61:	41 57                	push   %r15
   26e63:	41 56                	push   %r14
   26e65:	41 55                	push   %r13
   26e67:	41 54                	push   %r12
   26e69:	53                   	push   %rbx
   26e6a:	48 81 ec 58 02 00 00 	sub    $0x258,%rsp
   26e71:	49 89 d6             	mov    %rdx,%r14
   26e74:	49 89 f7             	mov    %rsi,%r15
   26e77:	48 89 fb             	mov    %rdi,%rbx
   26e7a:	48 8b 06             	mov    (%rsi),%rax
   26e7d:	48 8b 4e 08          	mov    0x8(%rsi),%rcx
   26e81:	48 89 44 24 18       	mov    %rax,0x18(%rsp)
   26e86:	48 89 4c 24 20       	mov    %rcx,0x20(%rsp)
   26e8b:	48 c7 44 24 28 00 02 	movq   $0x200,0x28(%rsp)
   26e92:	00 00 
   26e94:	48 c7 44 24 30 00 00 	movq   $0x0,0x30(%rsp)
   26e9b:	00 00 
   26e9d:	4c 8d 64 24 38       	lea    0x38(%rsp),%r12
   26ea2:	ba 20 02 00 00       	mov    $0x220,%edx
   26ea7:	4c 89 e7             	mov    %r12,%rdi
   26eaa:	31 f6                	xor    %esi,%esi
   26eac:	ff 15 d6 5e 0b 00    	call   *0xb5ed6(%rip)        # dcd88 <ostd::startup::_KEEP_START+0x6b0>
   26eb2:	49 8b 57 10          	mov    0x10(%r15),%rdx
   26eb6:	49 8b 4f 18          	mov    0x18(%r15),%rcx
   26eba:	48 83 ec 08          	sub    $0x8,%rsp
   26ebe:	48 8d 7c 24 10       	lea    0x10(%rsp),%rdi
   26ec3:	4c 8d 4c 24 20       	lea    0x20(%rsp),%r9
   26ec8:	be 05 00 00 00       	mov    $0x5,%esi
   26ecd:	41 b8 01 00 00 00    	mov    $0x1,%r8d
   26ed3:	68 20 02 00 00       	push   $0x220
   26ed8:	41 54                	push   %r12
   26eda:	6a 20                	push   $0x20
   26edc:	e8 ef 63 fe ff       	call   d2d0 <ostd::session::kernel_call>
   26ee1:	48 83 c4 20          	add    $0x20,%rsp
   26ee5:	0f b7 6c 24 16       	movzwl 0x16(%rsp),%ebp
   26eea:	66 83 fd ff          	cmp    $0xffff,%bp
   26eee:	0f 84 c2 00 00 00    	je     26fb6 <<terminal_sdk::stream::Endpoint>::read+0x156>
   26ef4:	66 85 ed             	test   %bp,%bp
   26ef7:	0f 85 be 00 00 00    	jne    26fbb <<terminal_sdk::stream::Endpoint>::read+0x15b>
   26efd:	44 8b 7c 24 10       	mov    0x10(%rsp),%r15d
   26f02:	41 8d 47 e8          	lea    -0x18(%r15),%eax
   26f06:	3d 09 02 00 00       	cmp    $0x209,%eax
   26f0b:	0f 83 05 01 00 00    	jae    27016 <<terminal_sdk::stream::Endpoint>::read+0x1b6>
   26f11:	49 83 c7 e8          	add    $0xffffffffffffffe8,%r15
   26f15:	66 bd 03 00          	mov    $0x3,%bp
   26f19:	49 83 ff 04          	cmp    $0x4,%r15
   26f1d:	0f 82 98 00 00 00    	jb     26fbb <<terminal_sdk::stream::Endpoint>::read+0x15b>
   26f23:	49 83 ff 08          	cmp    $0x8,%r15
   26f27:	0f 82 8e 00 00 00    	jb     26fbb <<terminal_sdk::stream::Endpoint>::read+0x15b>
   26f2d:	83 7c 24 54 00       	cmpl   $0x0,0x54(%rsp)
   26f32:	0f 85 83 00 00 00    	jne    26fbb <<terminal_sdk::stream::Endpoint>::read+0x15b>
   26f38:	44 8b 64 24 50       	mov    0x50(%rsp),%r12d
   26f3d:	41 8d 44 24 ff       	lea    -0x1(%r12),%eax
   26f42:	3d ff 01 00 00       	cmp    $0x1ff,%eax
   26f47:	77 72                	ja     26fbb <<terminal_sdk::stream::Endpoint>::read+0x15b>
   26f49:	4d 8d 6c 24 08       	lea    0x8(%r12),%r13
   26f4e:	4d 39 fd             	cmp    %r15,%r13
   26f51:	77 68                	ja     26fbb <<terminal_sdk::stream::Endpoint>::read+0x15b>
   26f53:	48 8d 74 24 58       	lea    0x58(%rsp),%rsi
   26f58:	4c 89 f7             	mov    %r14,%rdi
   26f5b:	4c 89 e2             	mov    %r12,%rdx
   26f5e:	ff 15 4c 5e 0b 00    	call   *0xb5e4c(%rip)        # dcdb0 <ostd::startup::_KEEP_START+0x6d8>
   26f64:	4d 39 fd             	cmp    %r15,%r13
   26f67:	75 52                	jne    26fbb <<terminal_sdk::stream::Endpoint>::read+0x15b>
   26f69:	4c 89 63 08          	mov    %r12,0x8(%rbx)
   26f6d:	66 c7 03 00 00       	movw   $0x0,(%rbx)
   26f72:	b8 07 00 00 00       	mov    $0x7,%eax
   26f77:	66 0f 1f 84 00 00 00 	nopw   0x0(%rax,%rax,1)
   26f7e:	00 00 
   26f80:	c6 44 04 31 00       	movb   $0x0,0x31(%rsp,%rax,1)
   26f85:	c6 44 04 32 00       	movb   $0x0,0x32(%rsp,%rax,1)
   26f8a:	c6 44 04 33 00       	movb   $0x0,0x33(%rsp,%rax,1)
   26f8f:	c6 44 04 34 00       	movb   $0x0,0x34(%rsp,%rax,1)
   26f94:	c6 44 04 35 00       	movb   $0x0,0x35(%rsp,%rax,1)
   26f99:	c6 44 04 36 00       	movb   $0x0,0x36(%rsp,%rax,1)
   26f9e:	c6 44 04 37 00       	movb   $0x0,0x37(%rsp,%rax,1)
   26fa3:	c6 44 04 38 00       	movb   $0x0,0x38(%rsp,%rax,1)
   26fa8:	48 83 c0 08          	add    $0x8,%rax
   26fac:	48 3d 27 02 00 00    	cmp    $0x227,%rax
   26fb2:	75 cc                	jne    26f80 <<terminal_sdk::stream::Endpoint>::read+0x120>
   26fb4:	eb 4e                	jmp    27004 <<terminal_sdk::stream::Endpoint>::read+0x1a4>
   26fb6:	0f b7 6c 24 08       	movzwl 0x8(%rsp),%ebp
   26fbb:	66 89 6b 02          	mov    %bp,0x2(%rbx)
   26fbf:	66 c7 03 01 00       	movw   $0x1,(%rbx)
   26fc4:	b8 07 00 00 00       	mov    $0x7,%eax
   26fc9:	0f 1f 80 00 00 00 00 	nopl   0x0(%rax)
   26fd0:	c6 44 04 31 00       	movb   $0x0,0x31(%rsp,%rax,1)
   26fd5:	c6 44 04 32 00       	movb   $0x0,0x32(%rsp,%rax,1)
   26fda:	c6 44 04 33 00       	movb   $0x0,0x33(%rsp,%rax,1)
   26fdf:	c6 44 04 34 00       	movb   $0x0,0x34(%rsp,%rax,1)
   26fe4:	c6 44 04 35 00       	movb   $0x0,0x35(%rsp,%rax,1)
   26fe9:	c6 44 04 36 00       	movb   $0x0,0x36(%rsp,%rax,1)
   26fee:	c6 44 04 37 00       	movb   $0x0,0x37(%rsp,%rax,1)
   26ff3:	c6 44 04 38 00       	movb   $0x0,0x38(%rsp,%rax,1)
   26ff8:	48 83 c0 08          	add    $0x8,%rax
   26ffc:	48 3d 27 02 00 00    	cmp    $0x227,%rax
   27002:	75 cc                	jne    26fd0 <<terminal_sdk::stream::Endpoint>::read+0x170>
   27004:	48 81 c4 58 02 00 00 	add    $0x258,%rsp
   2700b:	5b                   	pop    %rbx
   2700c:	41 5c                	pop    %r12
   2700e:	41 5d                	pop    %r13
   27010:	41 5e                	pop    %r14
   27012:	41 5f                	pop    %r15
   27014:	5d                   	pop    %rbp
   27015:	c3                   	ret
   27016:	48 8d 0d e3 5a 0b 00 	lea    0xb5ae3(%rip),%rcx        # dcb00 <ostd::startup::_KEEP_START+0x428>
   2701d:	bf 18 00 00 00       	mov    $0x18,%edi
   27022:	ba 20 02 00 00       	mov    $0x220,%edx
   27027:	4c 89 fe             	mov    %r15,%rsi
   2702a:	e8 91 1d fe ff       	call   8dc0 <core::slice::index::slice_index_fail>
