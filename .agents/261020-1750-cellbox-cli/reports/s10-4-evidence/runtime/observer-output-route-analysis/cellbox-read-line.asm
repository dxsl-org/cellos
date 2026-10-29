
/tmp/cellbox-s10-3d-resume/cand/desktop/files/cellbox:     file format elf64-x86-64


Disassembly of section .text:

0000000000019ec0 <cellbox::executor::terminal_read_line>:
   19ec0:	55                   	push   %rbp
   19ec1:	41 57                	push   %r15
   19ec3:	41 56                	push   %r14
   19ec5:	41 55                	push   %r13
   19ec7:	41 54                	push   %r12
   19ec9:	53                   	push   %rbx
   19eca:	48 81 ec 88 01 00 00 	sub    $0x188,%rsp
   19ed1:	49 89 f6             	mov    %rsi,%r14
   19ed4:	48 89 fb             	mov    %rdi,%rbx
   19ed7:	80 3d 12 e2 98 00 00 	cmpb   $0x0,0x98e212(%rip)        # 9a80f0 <ostd::heap::HEAP_INIT>
   19ede:	75 67                	jne    19f47 <cellbox::executor::terminal_read_line+0x87>
   19ee0:	48 c7 05 d5 e1 88 00 	movq   $0x0,0x88e1d5(%rip)        # 8a80c0 <ostd::heap::HEAP>
   19ee7:	00 00 00 00 
   19eeb:	b0 01                	mov    $0x1,%al
   19eed:	84 c0                	test   %al,%al
   19eef:	0f 84 1a 0d 00 00    	je     1ac0f <cellbox::executor::terminal_read_line+0xd4f>
   19ef5:	48 c7 05 f0 e1 88 00 	movq   $0x100000,0x88e1f0(%rip)        # 8a80f0 <ostd::heap::HEAP_MEM>
   19efc:	00 00 10 00 
   19f00:	48 8d 05 e9 e1 98 00 	lea    0x98e1e9(%rip),%rax        # 9a80f0 <ostd::heap::HEAP_INIT>
   19f07:	48 8d 0d e2 e1 88 00 	lea    0x88e1e2(%rip),%rcx        # 8a80f0 <ostd::heap::HEAP_MEM>
   19f0e:	48 c7 05 df e1 88 00 	movq   $0x0,0x88e1df(%rip)        # 8a80f8 <ostd::heap::HEAP_MEM+0x8>
   19f15:	00 00 00 00 
   19f19:	48 c7 05 a4 e1 88 00 	movq   $0x0,0x88e1a4(%rip)        # 8a80c8 <ostd::heap::HEAP+0x8>
   19f20:	00 00 00 00 
   19f24:	48 89 0d a5 e1 88 00 	mov    %rcx,0x88e1a5(%rip)        # 8a80d0 <ostd::heap::HEAP+0x10>
   19f2b:	48 89 0d a6 e1 88 00 	mov    %rcx,0x88e1a6(%rip)        # 8a80d8 <ostd::heap::HEAP+0x18>
   19f32:	48 89 05 a7 e1 88 00 	mov    %rax,0x88e1a7(%rip)        # 8a80e0 <ostd::heap::HEAP+0x20>
   19f39:	c6 05 a8 e1 88 00 00 	movb   $0x0,0x88e1a8(%rip)        # 8a80e8 <ostd::heap::HEAP+0x28>
   19f40:	c6 05 a9 e1 98 00 01 	movb   $0x1,0x98e1a9(%rip)        # 9a80f0 <ostd::heap::HEAP_INIT>
   19f47:	bf 01 00 00 00       	mov    $0x1,%edi
   19f4c:	be 00 10 00 00       	mov    $0x1000,%esi
   19f51:	e8 0a 90 0c 00       	call   e2f60 <<linked_list_allocator::Heap>::allocate_first_fit>
   19f56:	48 85 c0             	test   %rax,%rax
   19f59:	0f 84 ce 0c 00 00    	je     1ac2d <cellbox::executor::terminal_read_line+0xd6d>
   19f5f:	48 c7 44 24 10 00 10 	movq   $0x1000,0x10(%rsp)
   19f66:	00 00 
   19f68:	48 89 44 24 18       	mov    %rax,0x18(%rsp)
   19f6d:	48 c7 44 24 20 00 00 	movq   $0x0,0x20(%rsp)
   19f74:	00 00 
   19f76:	80 3d 73 e1 98 00 00 	cmpb   $0x0,0x98e173(%rip)        # 9a80f0 <ostd::heap::HEAP_INIT>
   19f7d:	75 67                	jne    19fe6 <cellbox::executor::terminal_read_line+0x126>
   19f7f:	48 c7 05 36 e1 88 00 	movq   $0x0,0x88e136(%rip)        # 8a80c0 <ostd::heap::HEAP>
   19f86:	00 00 00 00 
   19f8a:	b0 01                	mov    $0x1,%al
   19f8c:	84 c0                	test   %al,%al
   19f8e:	0f 84 7b 0c 00 00    	je     1ac0f <cellbox::executor::terminal_read_line+0xd4f>
   19f94:	48 c7 05 51 e1 88 00 	movq   $0x100000,0x88e151(%rip)        # 8a80f0 <ostd::heap::HEAP_MEM>
   19f9b:	00 00 10 00 
   19f9f:	48 8d 05 4a e1 98 00 	lea    0x98e14a(%rip),%rax        # 9a80f0 <ostd::heap::HEAP_INIT>
   19fa6:	48 8d 0d 43 e1 88 00 	lea    0x88e143(%rip),%rcx        # 8a80f0 <ostd::heap::HEAP_MEM>
   19fad:	48 c7 05 40 e1 88 00 	movq   $0x0,0x88e140(%rip)        # 8a80f8 <ostd::heap::HEAP_MEM+0x8>
   19fb4:	00 00 00 00 
   19fb8:	48 c7 05 05 e1 88 00 	movq   $0x0,0x88e105(%rip)        # 8a80c8 <ostd::heap::HEAP+0x8>
   19fbf:	00 00 00 00 
   19fc3:	48 89 0d 06 e1 88 00 	mov    %rcx,0x88e106(%rip)        # 8a80d0 <ostd::heap::HEAP+0x10>
   19fca:	48 89 0d 07 e1 88 00 	mov    %rcx,0x88e107(%rip)        # 8a80d8 <ostd::heap::HEAP+0x18>
   19fd1:	48 89 05 08 e1 88 00 	mov    %rax,0x88e108(%rip)        # 8a80e0 <ostd::heap::HEAP+0x20>
   19fd8:	c6 05 09 e1 88 00 00 	movb   $0x0,0x88e109(%rip)        # 8a80e8 <ostd::heap::HEAP+0x28>
   19fdf:	c6 05 0a e1 98 00 01 	movb   $0x1,0x98e10a(%rip)        # 9a80f0 <ostd::heap::HEAP_INIT>
   19fe6:	48 89 5c 24 08       	mov    %rbx,0x8(%rsp)
   19feb:	bf 01 00 00 00       	mov    $0x1,%edi
   19ff0:	be 00 10 00 00       	mov    $0x1000,%esi
   19ff5:	e8 66 8f 0c 00       	call   e2f60 <<linked_list_allocator::Heap>::allocate_first_fit>
   19ffa:	48 85 c0             	test   %rax,%rax
   19ffd:	0f 84 2a 0c 00 00    	je     1ac2d <cellbox::executor::terminal_read_line+0xd6d>
   1a003:	48 b9 f8 ff ff ff ff 	movabs $0x7ffffffffffffff8,%rcx
   1a00a:	ff ff 7f 
   1a00d:	48 c7 44 24 30 00 10 	movq   $0x1000,0x30(%rsp)
   1a014:	00 00 
   1a016:	48 89 44 24 38       	mov    %rax,0x38(%rsp)
   1a01b:	48 c7 44 24 40 00 00 	movq   $0x0,0x40(%rsp)
   1a022:	00 00 
   1a024:	4c 89 74 24 28       	mov    %r14,0x28(%rsp)
   1a029:	49 8b 46 10          	mov    0x10(%r14),%rax
   1a02d:	48 89 44 24 48       	mov    %rax,0x48(%rsp)
   1a032:	41 b4 01             	mov    $0x1,%r12b
   1a035:	48 8d 41 08          	lea    0x8(%rcx),%rax
   1a039:	48 89 44 24 60       	mov    %rax,0x60(%rsp)
   1a03e:	c7 44 24 04 00 00 00 	movl   $0x0,0x4(%rsp)
   1a045:	00 
   1a046:	31 ed                	xor    %ebp,%ebp
   1a048:	eb 16                	jmp    1a060 <cellbox::executor::terminal_read_line+0x1a0>
   1a04a:	40 b5 01             	mov    $0x1,%bpl
   1a04d:	48 8d bc 24 d0 00 00 	lea    0xd0(%rsp),%rdi
   1a054:	00 
   1a055:	e8 06 68 fe ff       	call   860 <core::ptr::drop_in_place::<cellbox::executor::ScrubbedEvent>>
   1a05a:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
   1a060:	31 c0                	xor    %eax,%eax
   1a062:	f0 44 0f b0 25 e5 60 	lock cmpxchg %r12b,0x2660e5(%rip)        # 280150 <cellbox::executor::SERIAL_SESSION>
   1a069:	26 00 
   1a06b:	0f 85 01 01 00 00    	jne    1a172 <cellbox::executor::terminal_read_line+0x2b2>
   1a071:	83 3d e0 60 26 00 ff 	cmpl   $0xffffffff,0x2660e0(%rip)        # 280158 <cellbox::executor::SERIAL_SESSION+0x8>
   1a078:	0f 84 6d 08 00 00    	je     1a8eb <cellbox::executor::terminal_read_line+0xa2b>
   1a07e:	48 8d 7c 24 70       	lea    0x70(%rsp),%rdi
   1a083:	48 8d 35 ce 60 26 00 	lea    0x2660ce(%rip),%rsi        # 280158 <cellbox::executor::SERIAL_SESSION+0x8>
   1a08a:	48 c7 c2 ff ff ff ff 	mov    $0xffffffffffffffff,%rdx
   1a091:	e8 9a 21 18 00       	call   19c230 <<terminal_sdk::session::TerminalSession>::next_event>
   1a096:	48 8b 5c 24 70       	mov    0x70(%rsp),%rbx
   1a09b:	44 0f b7 74 24 78    	movzwl 0x78(%rsp),%r14d
   1a0a1:	48 83 fb ff          	cmp    $0xffffffffffffffff,%rbx
   1a0a5:	0f 84 4d 08 00 00    	je     1a8f8 <cellbox::executor::terminal_read_line+0xa38>
   1a0ab:	ba 56 00 00 00       	mov    $0x56,%edx
   1a0b0:	4c 8d ac 24 32 01 00 	lea    0x132(%rsp),%r13
   1a0b7:	00 
   1a0b8:	4c 89 ef             	mov    %r13,%rdi
   1a0bb:	48 8d 74 24 7a       	lea    0x7a(%rsp),%rsi
   1a0c0:	4c 8b 3d a1 d4 28 00 	mov    0x28d4a1(%rip),%r15        # 2a7568 <memchr::arch::x86_64::memchr::memchr_raw::FN+0x280>
   1a0c7:	41 ff d7             	call   *%r15
   1a0ca:	c6 05 7f 60 26 00 00 	movb   $0x0,0x26607f(%rip)        # 280150 <cellbox::executor::SERIAL_SESSION>
   1a0d1:	48 89 9c 24 d0 00 00 	mov    %rbx,0xd0(%rsp)
   1a0d8:	00 
   1a0d9:	66 44 89 b4 24 d8 00 	mov    %r14w,0xd8(%rsp)
   1a0e0:	00 00 
   1a0e2:	ba 56 00 00 00       	mov    $0x56,%edx
   1a0e7:	48 8d bc 24 da 00 00 	lea    0xda(%rsp),%rdi
   1a0ee:	00 
   1a0ef:	4c 89 ee             	mov    %r13,%rsi
   1a0f2:	41 ff d7             	call   *%r15
   1a0f5:	48 89 d8             	mov    %rbx,%rax
   1a0f8:	48 33 44 24 60       	xor    0x60(%rsp),%rax
   1a0fd:	48 85 db             	test   %rbx,%rbx
   1a100:	b9 08 00 00 00       	mov    $0x8,%ecx
   1a105:	48 0f 49 c1          	cmovns %rcx,%rax
   1a109:	48 83 f8 07          	cmp    $0x7,%rax
   1a10d:	0f 87 3a ff ff ff    	ja     1a04d <cellbox::executor::terminal_read_line+0x18d>
   1a113:	48 8d 0d 12 81 20 00 	lea    0x208112(%rip),%rcx        # 22222c <__floatsidf+0x2f39c>
   1a11a:	48 63 04 81          	movslq (%rcx,%rax,4),%rax
   1a11e:	48 01 c8             	add    %rcx,%rax
   1a121:	ff e0                	jmp    *%rax
   1a123:	0f b6 84 24 e0 00 00 	movzbl 0xe0(%rsp),%eax
   1a12a:	00 
   1a12b:	83 f8 01             	cmp    $0x1,%eax
   1a12e:	74 09                	je     1a139 <cellbox::executor::terminal_read_line+0x279>
   1a130:	83 f8 02             	cmp    $0x2,%eax
   1a133:	0f 85 14 ff ff ff    	jne    1a04d <cellbox::executor::terminal_read_line+0x18d>
   1a139:	40 f6 c5 01          	test   $0x1,%bpl
   1a13d:	0f 85 07 ff ff ff    	jne    1a04a <cellbox::executor::terminal_read_line+0x18a>
   1a143:	8b 84 24 d8 00 00 00 	mov    0xd8(%rsp),%eax
   1a14a:	8b 8c 24 dc 00 00 00 	mov    0xdc(%rsp),%ecx
   1a151:	3d 01 00 11 00       	cmp    $0x110001,%eax
   1a156:	0f 85 50 01 00 00    	jne    1a2ac <cellbox::executor::terminal_read_line+0x3ec>
   1a15c:	85 c9                	test   %ecx,%ecx
   1a15e:	0f 85 62 03 00 00    	jne    1a4c6 <cellbox::executor::terminal_read_line+0x606>
   1a164:	e9 55 01 00 00       	jmp    1a2be <cellbox::executor::terminal_read_line+0x3fe>
   1a169:	0f 1f 80 00 00 00 00 	nopl   0x0(%rax)
   1a170:	f3 90                	pause
   1a172:	0f b6 05 d7 5f 26 00 	movzbl 0x265fd7(%rip),%eax        # 280150 <cellbox::executor::SERIAL_SESSION>
   1a179:	84 c0                	test   %al,%al
   1a17b:	75 f3                	jne    1a170 <cellbox::executor::terminal_read_line+0x2b0>
   1a17d:	e9 de fe ff ff       	jmp    1a060 <cellbox::executor::terminal_read_line+0x1a0>
   1a182:	8b 84 24 d8 00 00 00 	mov    0xd8(%rsp),%eax
   1a189:	83 f8 04             	cmp    $0x4,%eax
   1a18c:	74 79                	je     1a207 <cellbox::executor::terminal_read_line+0x347>
   1a18e:	83 f8 05             	cmp    $0x5,%eax
   1a191:	0f 85 8f 00 00 00    	jne    1a226 <cellbox::executor::terminal_read_line+0x366>
   1a197:	b8 0b 00 00 00       	mov    $0xb,%eax
   1a19c:	be 24 00 00 00       	mov    $0x24,%esi
   1a1a1:	48 8d 3d 19 41 21 00 	lea    0x214119(%rip),%rdi        # 22e2c1 <__floatsidf+0x3b431>
   1a1a8:	31 d2                	xor    %edx,%edx
   1a1aa:	45 31 d2             	xor    %r10d,%r10d
   1a1ad:	0f 05                	syscall
   1a1af:	be 0e 00 00 00       	mov    $0xe,%esi
   1a1b4:	48 8d 3d d0 40 21 00 	lea    0x2140d0(%rip),%rdi        # 22e28b <__floatsidf+0x3b3fb>
   1a1bb:	31 d2                	xor    %edx,%edx
   1a1bd:	e8 1e da ff ff       	call   17be0 <cellbox::executor::terminal_write>
   1a1c2:	66 83 f8 ff          	cmp    $0xffff,%ax
   1a1c6:	0f 85 71 07 00 00    	jne    1a93d <cellbox::executor::terminal_read_line+0xa7d>
   1a1cc:	48 8b 7c 24 18       	mov    0x18(%rsp),%rdi
   1a1d1:	48 8b 74 24 20       	mov    0x20(%rsp),%rsi
   1a1d6:	e9 cd 02 00 00       	jmp    1a4a8 <cellbox::executor::terminal_read_line+0x5e8>
   1a1db:	f6 44 24 04 01       	testb  $0x1,0x4(%rsp)
   1a1e0:	75 1a                	jne    1a1fc <cellbox::executor::terminal_read_line+0x33c>
   1a1e2:	4c 8b bc 24 e8 00 00 	mov    0xe8(%rsp),%r15
   1a1e9:	00 
   1a1ea:	48 8b 74 24 20       	mov    0x20(%rsp),%rsi
   1a1ef:	4a 8d 1c 3e          	lea    (%rsi,%r15,1),%rbx
   1a1f3:	48 81 fb 00 10 00 00 	cmp    $0x1000,%rbx
   1a1fa:	76 65                	jbe    1a261 <cellbox::executor::terminal_read_line+0x3a1>
   1a1fc:	b0 01                	mov    $0x1,%al
   1a1fe:	89 44 24 04          	mov    %eax,0x4(%rsp)
   1a202:	e9 46 fe ff ff       	jmp    1a04d <cellbox::executor::terminal_read_line+0x18d>
   1a207:	31 ed                	xor    %ebp,%ebp
   1a209:	b8 0b 00 00 00       	mov    $0xb,%eax
   1a20e:	be 28 00 00 00       	mov    $0x28,%esi
   1a213:	48 8d 3d 7f 40 21 00 	lea    0x21407f(%rip),%rdi        # 22e299 <__floatsidf+0x3b409>
   1a21a:	31 d2                	xor    %edx,%edx
   1a21c:	45 31 d2             	xor    %r10d,%r10d
   1a21f:	0f 05                	syscall
   1a221:	e9 27 fe ff ff       	jmp    1a04d <cellbox::executor::terminal_read_line+0x18d>
   1a226:	48 8d 84 24 d8 00 00 	lea    0xd8(%rsp),%rax
   1a22d:	00 
   1a22e:	48 89 44 24 68       	mov    %rax,0x68(%rsp)
   1a233:	48 8b 54 24 10       	mov    0x10(%rsp),%rdx
   1a238:	48 85 d2             	test   %rdx,%rdx
   1a23b:	48 8b 7c 24 28       	mov    0x28(%rsp),%rdi
   1a240:	0f 84 16 01 00 00    	je     1a35c <cellbox::executor::terminal_read_line+0x49c>
   1a246:	48 8b 44 24 18       	mov    0x18(%rsp),%rax
   1a24b:	89 d1                	mov    %edx,%ecx
   1a24d:	83 e1 07             	and    $0x7,%ecx
   1a250:	48 83 fa 08          	cmp    $0x8,%rdx
   1a254:	0f 83 9d 00 00 00    	jae    1a2f7 <cellbox::executor::terminal_read_line+0x437>
   1a25a:	31 f6                	xor    %esi,%esi
   1a25c:	e9 e4 00 00 00       	jmp    1a345 <cellbox::executor::terminal_read_line+0x485>
   1a261:	4c 8b b4 24 e0 00 00 	mov    0xe0(%rsp),%r14
   1a268:	00 
   1a269:	48 8b 44 24 10       	mov    0x10(%rsp),%rax
   1a26e:	48 29 f0             	sub    %rsi,%rax
   1a271:	49 39 c7             	cmp    %rax,%r15
   1a274:	0f 87 27 06 00 00    	ja     1a8a1 <cellbox::executor::terminal_read_line+0x9e1>
   1a27a:	4d 85 ff             	test   %r15,%r15
   1a27d:	0f 84 13 02 00 00    	je     1a496 <cellbox::executor::terminal_read_line+0x5d6>
   1a283:	48 03 74 24 18       	add    0x18(%rsp),%rsi
   1a288:	48 89 f7             	mov    %rsi,%rdi
   1a28b:	4c 89 f6             	mov    %r14,%rsi
   1a28e:	4c 89 fa             	mov    %r15,%rdx
   1a291:	ff 15 d1 d2 28 00    	call   *0x28d2d1(%rip)        # 2a7568 <memchr::arch::x86_64::memchr::memchr_raw::FN+0x280>
   1a297:	4c 8b b4 24 e0 00 00 	mov    0xe0(%rsp),%r14
   1a29e:	00 
   1a29f:	48 8b b4 24 e8 00 00 	mov    0xe8(%rsp),%rsi
   1a2a6:	00 
   1a2a7:	e9 ec 01 00 00       	jmp    1a498 <cellbox::executor::terminal_read_line+0x5d8>
   1a2ac:	89 c2                	mov    %eax,%edx
   1a2ae:	83 f2 6a             	xor    $0x6a,%edx
   1a2b1:	89 ce                	mov    %ecx,%esi
   1a2b3:	83 f6 02             	xor    $0x2,%esi
   1a2b6:	09 d6                	or     %edx,%esi
   1a2b8:	0f 85 00 02 00 00    	jne    1a4be <cellbox::executor::terminal_read_line+0x5fe>
   1a2be:	f6 44 24 04 01       	testb  $0x1,0x4(%rsp)
   1a2c3:	0f 84 ff 07 00 00    	je     1aac8 <cellbox::executor::terminal_read_line+0xc08>
   1a2c9:	48 8b 54 24 10       	mov    0x10(%rsp),%rdx
   1a2ce:	48 85 d2             	test   %rdx,%rdx
   1a2d1:	48 8b 7c 24 28       	mov    0x28(%rsp),%rdi
   1a2d6:	0f 84 50 02 00 00    	je     1a52c <cellbox::executor::terminal_read_line+0x66c>
   1a2dc:	48 8b 44 24 18       	mov    0x18(%rsp),%rax
   1a2e1:	89 d1                	mov    %edx,%ecx
   1a2e3:	83 e1 07             	and    $0x7,%ecx
   1a2e6:	48 83 fa 08          	cmp    $0x8,%rdx
   1a2ea:	0f 83 dd 01 00 00    	jae    1a4cd <cellbox::executor::terminal_read_line+0x60d>
   1a2f0:	31 f6                	xor    %esi,%esi
   1a2f2:	e9 1e 02 00 00       	jmp    1a515 <cellbox::executor::terminal_read_line+0x655>
   1a2f7:	48 be f8 ff ff ff ff 	movabs $0x7ffffffffffffff8,%rsi
   1a2fe:	ff ff 7f 
   1a301:	48 21 f2             	and    %rsi,%rdx
   1a304:	31 f6                	xor    %esi,%esi
   1a306:	66 2e 0f 1f 84 00 00 	cs nopw 0x0(%rax,%rax,1)
   1a30d:	00 00 00 
   1a310:	c6 04 30 00          	movb   $0x0,(%rax,%rsi,1)
   1a314:	c6 44 30 01 00       	movb   $0x0,0x1(%rax,%rsi,1)
   1a319:	c6 44 30 02 00       	movb   $0x0,0x2(%rax,%rsi,1)
   1a31e:	c6 44 30 03 00       	movb   $0x0,0x3(%rax,%rsi,1)
   1a323:	c6 44 30 04 00       	movb   $0x0,0x4(%rax,%rsi,1)
   1a328:	c6 44 30 05 00       	movb   $0x0,0x5(%rax,%rsi,1)
   1a32d:	c6 44 30 06 00       	movb   $0x0,0x6(%rax,%rsi,1)
   1a332:	c6 44 30 07 00       	movb   $0x0,0x7(%rax,%rsi,1)
   1a337:	48 83 c6 08          	add    $0x8,%rsi
   1a33b:	48 39 f2             	cmp    %rsi,%rdx
   1a33e:	75 d0                	jne    1a310 <cellbox::executor::terminal_read_line+0x450>
   1a340:	48 85 c9             	test   %rcx,%rcx
   1a343:	74 17                	je     1a35c <cellbox::executor::terminal_read_line+0x49c>
   1a345:	48 01 f0             	add    %rsi,%rax
   1a348:	31 d2                	xor    %edx,%edx
   1a34a:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
   1a350:	c6 04 10 00          	movb   $0x0,(%rax,%rdx,1)
   1a354:	48 ff c2             	inc    %rdx
   1a357:	48 39 d1             	cmp    %rdx,%rcx
   1a35a:	75 f4                	jne    1a350 <cellbox::executor::terminal_read_line+0x490>
   1a35c:	48 c7 44 24 20 00 00 	movq   $0x0,0x20(%rsp)
   1a363:	00 00 
   1a365:	48 8b 54 24 30       	mov    0x30(%rsp),%rdx
   1a36a:	48 85 d2             	test   %rdx,%rdx
   1a36d:	74 7d                	je     1a3ec <cellbox::executor::terminal_read_line+0x52c>
   1a36f:	48 8b 44 24 38       	mov    0x38(%rsp),%rax
   1a374:	89 d1                	mov    %edx,%ecx
   1a376:	83 e1 07             	and    $0x7,%ecx
   1a379:	48 83 fa 08          	cmp    $0x8,%rdx
   1a37d:	73 04                	jae    1a383 <cellbox::executor::terminal_read_line+0x4c3>
   1a37f:	31 f6                	xor    %esi,%esi
   1a381:	eb 52                	jmp    1a3d5 <cellbox::executor::terminal_read_line+0x515>
   1a383:	48 be f8 ff ff ff ff 	movabs $0x7ffffffffffffff8,%rsi
   1a38a:	ff ff 7f 
   1a38d:	48 21 f2             	and    %rsi,%rdx
   1a390:	31 f6                	xor    %esi,%esi
   1a392:	66 66 66 66 66 2e 0f 	data16 data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
   1a399:	1f 84 00 00 00 00 00 
   1a3a0:	c6 04 30 00          	movb   $0x0,(%rax,%rsi,1)
   1a3a4:	c6 44 30 01 00       	movb   $0x0,0x1(%rax,%rsi,1)
   1a3a9:	c6 44 30 02 00       	movb   $0x0,0x2(%rax,%rsi,1)
   1a3ae:	c6 44 30 03 00       	movb   $0x0,0x3(%rax,%rsi,1)
   1a3b3:	c6 44 30 04 00       	movb   $0x0,0x4(%rax,%rsi,1)
   1a3b8:	c6 44 30 05 00       	movb   $0x0,0x5(%rax,%rsi,1)
   1a3bd:	c6 44 30 06 00       	movb   $0x0,0x6(%rax,%rsi,1)
   1a3c2:	c6 44 30 07 00       	movb   $0x0,0x7(%rax,%rsi,1)
   1a3c7:	48 83 c6 08          	add    $0x8,%rsi
   1a3cb:	48 39 f2             	cmp    %rsi,%rdx
   1a3ce:	75 d0                	jne    1a3a0 <cellbox::executor::terminal_read_line+0x4e0>
   1a3d0:	48 85 c9             	test   %rcx,%rcx
   1a3d3:	74 17                	je     1a3ec <cellbox::executor::terminal_read_line+0x52c>
   1a3d5:	48 01 f0             	add    %rsi,%rax
   1a3d8:	31 d2                	xor    %edx,%edx
   1a3da:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
   1a3e0:	c6 04 10 00          	movb   $0x0,(%rax,%rdx,1)
   1a3e4:	48 ff c2             	inc    %rdx
   1a3e7:	48 39 d1             	cmp    %rdx,%rcx
   1a3ea:	75 f4                	jne    1a3e0 <cellbox::executor::terminal_read_line+0x520>
   1a3ec:	48 c7 44 24 40 00 00 	movq   $0x0,0x40(%rsp)
   1a3f3:	00 00 
   1a3f5:	48 8b 5f 10          	mov    0x10(%rdi),%rbx
   1a3f9:	48 8d 44 24 68       	lea    0x68(%rsp),%rax
   1a3fe:	48 89 44 24 50       	mov    %rax,0x50(%rsp)
   1a403:	48 8d 05 86 4b 17 00 	lea    0x174b86(%rip),%rax        # 18ef90 <<&sunset::channel::ChanNum as core::fmt::Display>::fmt>
   1a40a:	48 89 44 24 58       	mov    %rax,0x58(%rsp)
   1a40f:	48 8d 7c 24 70       	lea    0x70(%rsp),%rdi
   1a414:	48 8d 35 b5 2e 21 00 	lea    0x212eb5(%rip),%rsi        # 22d2d0 <__floatsidf+0x3a440>
   1a41b:	48 8d 54 24 50       	lea    0x50(%rsp),%rdx
   1a420:	e8 eb c0 01 00       	call   36510 <alloc::fmt::format::format_inner>
   1a425:	4c 8b 74 24 70       	mov    0x70(%rsp),%r14
   1a42a:	4c 8b 7c 24 78       	mov    0x78(%rsp),%r15
   1a42f:	48 8b b4 24 80 00 00 	mov    0x80(%rsp),%rsi
   1a436:	00 
   1a437:	4c 89 ff             	mov    %r15,%rdi
   1a43a:	e8 41 82 ff ff       	call   12680 <cellbox::executor::shell_stderr>
   1a43f:	be 01 00 00 00       	mov    $0x1,%esi
   1a444:	48 8d 3d fb 2c 26 00 	lea    0x262cfb(%rip),%rdi        # 27d146 <core::unicode::unicode_data::white_space::WHITESPACE_MAP+0x27d>
   1a44b:	e8 30 82 ff ff       	call   12680 <cellbox::executor::shell_stderr>
   1a450:	4d 85 f6             	test   %r14,%r14
   1a453:	74 10                	je     1a465 <cellbox::executor::terminal_read_line+0x5a5>
   1a455:	be 01 00 00 00       	mov    $0x1,%esi
   1a45a:	4c 89 ff             	mov    %r15,%rdi
   1a45d:	4c 89 f2             	mov    %r14,%rdx
   1a460:	e8 bb 88 0c 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   1a465:	be 09 00 00 00       	mov    $0x9,%esi
   1a46a:	48 8d 3d 74 8c 21 00 	lea    0x218c74(%rip),%rdi        # 2330e5 <unicode_width::tables::EMOJI_MODIFIER_LEAF_7+0x1911>
   1a471:	31 d2                	xor    %edx,%edx
   1a473:	e8 68 d7 ff ff       	call   17be0 <cellbox::executor::terminal_write>
   1a478:	66 83 f8 ff          	cmp    $0xffff,%ax
   1a47c:	0f 85 bb 04 00 00    	jne    1a93d <cellbox::executor::terminal_read_line+0xa7d>
   1a482:	48 89 5c 24 48       	mov    %rbx,0x48(%rsp)
   1a487:	c7 44 24 04 00 00 00 	movl   $0x0,0x4(%rsp)
   1a48e:	00 
   1a48f:	31 ed                	xor    %ebp,%ebp
   1a491:	e9 b7 fb ff ff       	jmp    1a04d <cellbox::executor::terminal_read_line+0x18d>
   1a496:	31 f6                	xor    %esi,%esi
   1a498:	48 89 5c 24 20       	mov    %rbx,0x20(%rsp)
   1a49d:	c7 44 24 04 00 00 00 	movl   $0x0,0x4(%rsp)
   1a4a4:	00 
   1a4a5:	4c 89 f7             	mov    %r14,%rdi
   1a4a8:	31 d2                	xor    %edx,%edx
   1a4aa:	e8 31 d7 ff ff       	call   17be0 <cellbox::executor::terminal_write>
   1a4af:	66 83 f8 ff          	cmp    $0xffff,%ax
   1a4b3:	0f 84 94 fb ff ff    	je     1a04d <cellbox::executor::terminal_read_line+0x18d>
   1a4b9:	e9 7f 04 00 00       	jmp    1a93d <cellbox::executor::terminal_read_line+0xa7d>
   1a4be:	85 c9                	test   %ecx,%ecx
   1a4c0:	0f 84 15 01 00 00    	je     1a5db <cellbox::executor::terminal_read_line+0x71b>
   1a4c6:	31 ed                	xor    %ebp,%ebp
   1a4c8:	e9 80 fb ff ff       	jmp    1a04d <cellbox::executor::terminal_read_line+0x18d>
   1a4cd:	48 be f8 ff ff ff ff 	movabs $0x7ffffffffffffff8,%rsi
   1a4d4:	ff ff 7f 
   1a4d7:	48 21 f2             	and    %rsi,%rdx
   1a4da:	31 f6                	xor    %esi,%esi
   1a4dc:	0f 1f 40 00          	nopl   0x0(%rax)
   1a4e0:	c6 04 30 00          	movb   $0x0,(%rax,%rsi,1)
   1a4e4:	c6 44 30 01 00       	movb   $0x0,0x1(%rax,%rsi,1)
   1a4e9:	c6 44 30 02 00       	movb   $0x0,0x2(%rax,%rsi,1)
   1a4ee:	c6 44 30 03 00       	movb   $0x0,0x3(%rax,%rsi,1)
   1a4f3:	c6 44 30 04 00       	movb   $0x0,0x4(%rax,%rsi,1)
   1a4f8:	c6 44 30 05 00       	movb   $0x0,0x5(%rax,%rsi,1)
   1a4fd:	c6 44 30 06 00       	movb   $0x0,0x6(%rax,%rsi,1)
   1a502:	c6 44 30 07 00       	movb   $0x0,0x7(%rax,%rsi,1)
   1a507:	48 83 c6 08          	add    $0x8,%rsi
   1a50b:	48 39 f2             	cmp    %rsi,%rdx
   1a50e:	75 d0                	jne    1a4e0 <cellbox::executor::terminal_read_line+0x620>
   1a510:	48 85 c9             	test   %rcx,%rcx
   1a513:	74 17                	je     1a52c <cellbox::executor::terminal_read_line+0x66c>
   1a515:	48 01 f0             	add    %rsi,%rax
   1a518:	31 d2                	xor    %edx,%edx
   1a51a:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
   1a520:	c6 04 10 00          	movb   $0x0,(%rax,%rdx,1)
   1a524:	48 ff c2             	inc    %rdx
   1a527:	48 39 d1             	cmp    %rdx,%rcx
   1a52a:	75 f4                	jne    1a520 <cellbox::executor::terminal_read_line+0x660>
   1a52c:	48 c7 44 24 20 00 00 	movq   $0x0,0x20(%rsp)
   1a533:	00 00 
   1a535:	48 8b 47 10          	mov    0x10(%rdi),%rax
   1a539:	48 89 44 24 48       	mov    %rax,0x48(%rsp)
   1a53e:	48 8d 05 db 40 21 00 	lea    0x2140db(%rip),%rax        # 22e620 <__floatsidf+0x3b790>
   1a545:	48 89 44 24 50       	mov    %rax,0x50(%rsp)
   1a54a:	48 8d 05 df 12 0b 00 	lea    0xb12df(%rip),%rax        # cb830 <<u64 as core::fmt::Display>::fmt>
   1a551:	48 89 44 24 58       	mov    %rax,0x58(%rsp)
   1a556:	48 8d 7c 24 70       	lea    0x70(%rsp),%rdi
   1a55b:	48 8d 35 b5 20 21 00 	lea    0x2120b5(%rip),%rsi        # 22c617 <__floatsidf+0x39787>
   1a562:	48 8d 54 24 50       	lea    0x50(%rsp),%rdx
   1a567:	e8 a4 bf 01 00       	call   36510 <alloc::fmt::format::format_inner>
   1a56c:	4c 8b 74 24 70       	mov    0x70(%rsp),%r14
   1a571:	4c 8b 7c 24 78       	mov    0x78(%rsp),%r15
   1a576:	48 8b b4 24 80 00 00 	mov    0x80(%rsp),%rsi
   1a57d:	00 
   1a57e:	4c 89 ff             	mov    %r15,%rdi
   1a581:	e8 fa 80 ff ff       	call   12680 <cellbox::executor::shell_stderr>
   1a586:	be 01 00 00 00       	mov    $0x1,%esi
   1a58b:	48 8d 3d b4 2b 26 00 	lea    0x262bb4(%rip),%rdi        # 27d146 <core::unicode::unicode_data::white_space::WHITESPACE_MAP+0x27d>
   1a592:	e8 e9 80 ff ff       	call   12680 <cellbox::executor::shell_stderr>
   1a597:	4d 85 f6             	test   %r14,%r14
   1a59a:	74 10                	je     1a5ac <cellbox::executor::terminal_read_line+0x6ec>
   1a59c:	be 01 00 00 00       	mov    $0x1,%esi
   1a5a1:	4c 89 ff             	mov    %r15,%rdi
   1a5a4:	4c 89 f2             	mov    %r14,%rdx
   1a5a7:	e8 74 87 0c 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   1a5ac:	c7 44 24 04 00 00 00 	movl   $0x0,0x4(%rsp)
   1a5b3:	00 
   1a5b4:	be 09 00 00 00       	mov    $0x9,%esi
   1a5b9:	48 8d 3d 25 8b 21 00 	lea    0x218b25(%rip),%rdi        # 2330e5 <unicode_width::tables::EMOJI_MODIFIER_LEAF_7+0x1911>
   1a5c0:	31 d2                	xor    %edx,%edx
   1a5c2:	e8 19 d6 ff ff       	call   17be0 <cellbox::executor::terminal_write>
   1a5c7:	bd 00 00 00 00       	mov    $0x0,%ebp
   1a5cc:	66 83 f8 ff          	cmp    $0xffff,%ax
   1a5d0:	0f 84 77 fa ff ff    	je     1a04d <cellbox::executor::terminal_read_line+0x18d>
   1a5d6:	e9 62 03 00 00       	jmp    1a93d <cellbox::executor::terminal_read_line+0xa7d>
   1a5db:	8d 88 f0 ff ee ff    	lea    -0x110010(%rax),%ecx
   1a5e1:	83 f9 02             	cmp    $0x2,%ecx
   1a5e4:	73 47                	jae    1a62d <cellbox::executor::terminal_read_line+0x76d>
   1a5e6:	48 8b 5c 24 28       	mov    0x28(%rsp),%rbx
   1a5eb:	4c 8b 73 10          	mov    0x10(%rbx),%r14
   1a5ef:	4c 8b 7c 24 48       	mov    0x48(%rsp),%r15
   1a5f4:	4d 39 f7             	cmp    %r14,%r15
   1a5f7:	0f 85 1c 01 00 00    	jne    1a719 <cellbox::executor::terminal_read_line+0x859>
   1a5fd:	48 8b 74 24 18       	mov    0x18(%rsp),%rsi
   1a602:	48 8b 54 24 20       	mov    0x20(%rsp),%rdx
   1a607:	48 8b 7c 24 30       	mov    0x30(%rsp),%rdi
   1a60c:	48 85 ff             	test   %rdi,%rdi
   1a60f:	0f 84 e7 00 00 00    	je     1a6fc <cellbox::executor::terminal_read_line+0x83c>
   1a615:	48 8b 44 24 38       	mov    0x38(%rsp),%rax
   1a61a:	89 f9                	mov    %edi,%ecx
   1a61c:	83 e1 07             	and    $0x7,%ecx
   1a61f:	48 83 ff 08          	cmp    $0x8,%rdi
   1a623:	73 6a                	jae    1a68f <cellbox::executor::terminal_read_line+0x7cf>
   1a625:	45 31 c0             	xor    %r8d,%r8d
   1a628:	e9 af 00 00 00       	jmp    1a6dc <cellbox::executor::terminal_read_line+0x81c>
   1a62d:	3d 03 00 11 00       	cmp    $0x110003,%eax
   1a632:	0f 85 8e fe ff ff    	jne    1a4c6 <cellbox::executor::terminal_read_line+0x606>
   1a638:	48 8b 44 24 20       	mov    0x20(%rsp),%rax
   1a63d:	48 85 c0             	test   %rax,%rax
   1a640:	0f 84 80 fe ff ff    	je     1a4c6 <cellbox::executor::terminal_read_line+0x606>
   1a646:	48 8b 7c 24 18       	mov    0x18(%rsp),%rdi
   1a64b:	48 c7 c1 ff ff ff ff 	mov    $0xffffffffffffffff,%rcx
   1a652:	80 7c 07 ff 00       	cmpb   $0x0,-0x1(%rdi,%rax,1)
   1a657:	0f 89 29 02 00 00    	jns    1a886 <cellbox::executor::terminal_read_line+0x9c6>
   1a65d:	0f b6 54 07 fe       	movzbl -0x2(%rdi,%rax,1),%edx
   1a662:	80 fa bf             	cmp    $0xbf,%dl
   1a665:	0f 8f eb 01 00 00    	jg     1a856 <cellbox::executor::terminal_read_line+0x996>
   1a66b:	0f b6 74 07 fd       	movzbl -0x3(%rdi,%rax,1),%esi
   1a670:	40 80 fe c0          	cmp    $0xc0,%sil
   1a674:	0f 8d e1 01 00 00    	jge    1a85b <cellbox::executor::terminal_read_line+0x99b>
   1a67a:	0f b6 7c 07 fc       	movzbl -0x4(%rdi,%rax,1),%edi
   1a67f:	83 e7 07             	and    $0x7,%edi
   1a682:	c1 e7 06             	shl    $0x6,%edi
   1a685:	83 e6 3f             	and    $0x3f,%esi
   1a688:	09 fe                	or     %edi,%esi
   1a68a:	e9 cf 01 00 00       	jmp    1a85e <cellbox::executor::terminal_read_line+0x99e>
   1a68f:	49 b8 f8 ff ff ff ff 	movabs $0x7ffffffffffffff8,%r8
   1a696:	ff ff 7f 
   1a699:	4c 21 c7             	and    %r8,%rdi
   1a69c:	45 31 c0             	xor    %r8d,%r8d
   1a69f:	42 c6 04 00 00       	movb   $0x0,(%rax,%r8,1)
   1a6a4:	42 c6 44 00 01 00    	movb   $0x0,0x1(%rax,%r8,1)
   1a6aa:	42 c6 44 00 02 00    	movb   $0x0,0x2(%rax,%r8,1)
   1a6b0:	42 c6 44 00 03 00    	movb   $0x0,0x3(%rax,%r8,1)
   1a6b6:	42 c6 44 00 04 00    	movb   $0x0,0x4(%rax,%r8,1)
   1a6bc:	42 c6 44 00 05 00    	movb   $0x0,0x5(%rax,%r8,1)
   1a6c2:	42 c6 44 00 06 00    	movb   $0x0,0x6(%rax,%r8,1)
   1a6c8:	42 c6 44 00 07 00    	movb   $0x0,0x7(%rax,%r8,1)
   1a6ce:	49 83 c0 08          	add    $0x8,%r8
   1a6d2:	4c 39 c7             	cmp    %r8,%rdi
   1a6d5:	75 c8                	jne    1a69f <cellbox::executor::terminal_read_line+0x7df>
   1a6d7:	48 85 c9             	test   %rcx,%rcx
   1a6da:	74 20                	je     1a6fc <cellbox::executor::terminal_read_line+0x83c>
   1a6dc:	4c 01 c0             	add    %r8,%rax
   1a6df:	31 ff                	xor    %edi,%edi
   1a6e1:	66 66 66 66 66 66 2e 	data16 data16 data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
   1a6e8:	0f 1f 84 00 00 00 00 
   1a6ef:	00 
   1a6f0:	c6 04 38 00          	movb   $0x0,(%rax,%rdi,1)
   1a6f4:	48 ff c7             	inc    %rdi
   1a6f7:	48 39 f9             	cmp    %rdi,%rcx
   1a6fa:	75 f4                	jne    1a6f0 <cellbox::executor::terminal_read_line+0x830>
   1a6fc:	48 c7 44 24 40 00 00 	movq   $0x0,0x40(%rsp)
   1a703:	00 00 
   1a705:	48 01 f2             	add    %rsi,%rdx
   1a708:	48 8d 7c 24 30       	lea    0x30(%rsp),%rdi
   1a70d:	e8 6e 1a 02 00       	call   3c180 <<alloc::vec::Vec<u8> as alloc::vec::spec_extend::SpecExtend<&u8, core::slice::iter::Iter<u8>>>::spec_extend>
   1a712:	8b 84 24 d8 00 00 00 	mov    0xd8(%rsp),%eax
   1a719:	3d 10 00 11 00       	cmp    $0x110010,%eax
   1a71e:	75 0a                	jne    1a72a <cellbox::executor::terminal_read_line+0x86a>
   1a720:	49 83 ff 01          	cmp    $0x1,%r15
   1a724:	49 83 d7 ff          	adc    $0xffffffffffffffff,%r15
   1a728:	eb 0a                	jmp    1a734 <cellbox::executor::terminal_read_line+0x874>
   1a72a:	49 ff c7             	inc    %r15
   1a72d:	4d 39 fe             	cmp    %r15,%r14
   1a730:	4d 0f 42 fe          	cmovb  %r14,%r15
   1a734:	4d 39 f7             	cmp    %r14,%r15
   1a737:	73 14                	jae    1a74d <cellbox::executor::terminal_read_line+0x88d>
   1a739:	48 8b 43 08          	mov    0x8(%rbx),%rax
   1a73d:	4b 8d 0c 7f          	lea    (%r15,%r15,2),%rcx
   1a741:	48 8b 6c c8 08       	mov    0x8(%rax,%rcx,8),%rbp
   1a746:	4c 8b 6c c8 10       	mov    0x10(%rax,%rcx,8),%r13
   1a74b:	eb 0a                	jmp    1a757 <cellbox::executor::terminal_read_line+0x897>
   1a74d:	48 8b 6c 24 38       	mov    0x38(%rsp),%rbp
   1a752:	4c 8b 6c 24 40       	mov    0x40(%rsp),%r13
   1a757:	48 8b 54 24 10       	mov    0x10(%rsp),%rdx
   1a75c:	48 85 d2             	test   %rdx,%rdx
   1a75f:	74 7b                	je     1a7dc <cellbox::executor::terminal_read_line+0x91c>
   1a761:	48 8b 44 24 18       	mov    0x18(%rsp),%rax
   1a766:	89 d1                	mov    %edx,%ecx
   1a768:	83 e1 07             	and    $0x7,%ecx
   1a76b:	48 83 fa 08          	cmp    $0x8,%rdx
   1a76f:	73 04                	jae    1a775 <cellbox::executor::terminal_read_line+0x8b5>
   1a771:	31 f6                	xor    %esi,%esi
   1a773:	eb 50                	jmp    1a7c5 <cellbox::executor::terminal_read_line+0x905>
   1a775:	48 be f8 ff ff ff ff 	movabs $0x7ffffffffffffff8,%rsi
   1a77c:	ff ff 7f 
   1a77f:	48 21 f2             	and    %rsi,%rdx
   1a782:	31 f6                	xor    %esi,%esi
   1a784:	66 66 66 2e 0f 1f 84 	data16 data16 cs nopw 0x0(%rax,%rax,1)
   1a78b:	00 00 00 00 00 
   1a790:	c6 04 30 00          	movb   $0x0,(%rax,%rsi,1)
   1a794:	c6 44 30 01 00       	movb   $0x0,0x1(%rax,%rsi,1)
   1a799:	c6 44 30 02 00       	movb   $0x0,0x2(%rax,%rsi,1)
   1a79e:	c6 44 30 03 00       	movb   $0x0,0x3(%rax,%rsi,1)
   1a7a3:	c6 44 30 04 00       	movb   $0x0,0x4(%rax,%rsi,1)
   1a7a8:	c6 44 30 05 00       	movb   $0x0,0x5(%rax,%rsi,1)
   1a7ad:	c6 44 30 06 00       	movb   $0x0,0x6(%rax,%rsi,1)
   1a7b2:	c6 44 30 07 00       	movb   $0x0,0x7(%rax,%rsi,1)
   1a7b7:	48 83 c6 08          	add    $0x8,%rsi
   1a7bb:	48 39 f2             	cmp    %rsi,%rdx
   1a7be:	75 d0                	jne    1a790 <cellbox::executor::terminal_read_line+0x8d0>
   1a7c0:	48 85 c9             	test   %rcx,%rcx
   1a7c3:	74 17                	je     1a7dc <cellbox::executor::terminal_read_line+0x91c>
   1a7c5:	48 01 f0             	add    %rsi,%rax
   1a7c8:	31 d2                	xor    %edx,%edx
   1a7ca:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
   1a7d0:	c6 04 10 00          	movb   $0x0,(%rax,%rdx,1)
   1a7d4:	48 ff c2             	inc    %rdx
   1a7d7:	48 39 d1             	cmp    %rdx,%rcx
   1a7da:	75 f4                	jne    1a7d0 <cellbox::executor::terminal_read_line+0x910>
   1a7dc:	48 c7 44 24 20 00 00 	movq   $0x0,0x20(%rsp)
   1a7e3:	00 00 
   1a7e5:	4c 3b 6c 24 10       	cmp    0x10(%rsp),%r13
   1a7ea:	0f 87 d7 00 00 00    	ja     1a8c7 <cellbox::executor::terminal_read_line+0xa07>
   1a7f0:	bb 00 00 00 00       	mov    $0x0,%ebx
   1a7f5:	41 be 00 00 00 00    	mov    $0x0,%r14d
   1a7fb:	4d 85 ed             	test   %r13,%r13
   1a7fe:	74 17                	je     1a817 <cellbox::executor::terminal_read_line+0x957>
   1a800:	48 8b 7c 24 18       	mov    0x18(%rsp),%rdi
   1a805:	48 01 df             	add    %rbx,%rdi
   1a808:	48 89 ee             	mov    %rbp,%rsi
   1a80b:	4c 89 ea             	mov    %r13,%rdx
   1a80e:	ff 15 54 cd 28 00    	call   *0x28cd54(%rip)        # 2a7568 <memchr::arch::x86_64::memchr::memchr_raw::FN+0x280>
   1a814:	49 89 de             	mov    %rbx,%r14
   1a817:	4d 01 ee             	add    %r13,%r14
   1a81a:	4c 89 74 24 20       	mov    %r14,0x20(%rsp)
   1a81f:	be 0e 00 00 00       	mov    $0xe,%esi
   1a824:	48 8d 3d 60 3a 21 00 	lea    0x213a60(%rip),%rdi        # 22e28b <__floatsidf+0x3b3fb>
   1a82b:	31 d2                	xor    %edx,%edx
   1a82d:	e8 ae d3 ff ff       	call   17be0 <cellbox::executor::terminal_write>
   1a832:	66 83 f8 ff          	cmp    $0xffff,%ax
   1a836:	0f 85 01 01 00 00    	jne    1a93d <cellbox::executor::terminal_read_line+0xa7d>
   1a83c:	4c 89 7c 24 48       	mov    %r15,0x48(%rsp)
   1a841:	48 8b 7c 24 18       	mov    0x18(%rsp),%rdi
   1a846:	c7 44 24 04 00 00 00 	movl   $0x0,0x4(%rsp)
   1a84d:	00 
   1a84e:	4c 89 f6             	mov    %r14,%rsi
   1a851:	e9 6a fd ff ff       	jmp    1a5c0 <cellbox::executor::terminal_read_line+0x700>
   1a856:	83 e2 1f             	and    $0x1f,%edx
   1a859:	eb 0b                	jmp    1a866 <cellbox::executor::terminal_read_line+0x9a6>
   1a85b:	83 e6 0f             	and    $0xf,%esi
   1a85e:	c1 e6 06             	shl    $0x6,%esi
   1a861:	83 e2 3f             	and    $0x3f,%edx
   1a864:	09 f2                	or     %esi,%edx
   1a866:	83 fa 02             	cmp    $0x2,%edx
   1a869:	72 1b                	jb     1a886 <cellbox::executor::terminal_read_line+0x9c6>
   1a86b:	48 c7 c1 fe ff ff ff 	mov    $0xfffffffffffffffe,%rcx
   1a872:	83 fa 20             	cmp    $0x20,%edx
   1a875:	72 0f                	jb     1a886 <cellbox::executor::terminal_read_line+0x9c6>
   1a877:	31 c9                	xor    %ecx,%ecx
   1a879:	81 fa 00 04 00 00    	cmp    $0x400,%edx
   1a87f:	0f 92 c1             	setb   %cl
   1a882:	48 83 c9 fc          	or     $0xfffffffffffffffc,%rcx
   1a886:	48 01 c1             	add    %rax,%rcx
   1a889:	48 89 4c 24 20       	mov    %rcx,0x20(%rsp)
   1a88e:	31 ed                	xor    %ebp,%ebp
   1a890:	be 03 00 00 00       	mov    $0x3,%esi
   1a895:	48 8d 3d 3c eb 21 00 	lea    0x21eb3c(%rip),%rdi        # 2393d8 <core::num::imp::dec2flt::table::POWER_OF_FIVE_128+0x2b90>
   1a89c:	e9 07 fc ff ff       	jmp    1a4a8 <cellbox::executor::terminal_read_line+0x5e8>
   1a8a1:	b9 01 00 00 00       	mov    $0x1,%ecx
   1a8a6:	41 b8 01 00 00 00    	mov    $0x1,%r8d
   1a8ac:	48 8d 7c 24 10       	lea    0x10(%rsp),%rdi
   1a8b1:	4c 89 fa             	mov    %r15,%rdx
   1a8b4:	e8 f7 f6 04 00       	call   69fb0 <<alloc::raw_vec::RawVecInner<_>>::reserve::do_reserve_and_handle::<alloc::alloc::Global>>
   1a8b9:	48 8b 74 24 20       	mov    0x20(%rsp),%rsi
   1a8be:	4a 8d 1c 3e          	lea    (%rsi,%r15,1),%rbx
   1a8c2:	e9 bc f9 ff ff       	jmp    1a283 <cellbox::executor::terminal_read_line+0x3c3>
   1a8c7:	b9 01 00 00 00       	mov    $0x1,%ecx
   1a8cc:	41 b8 01 00 00 00    	mov    $0x1,%r8d
   1a8d2:	48 8d 7c 24 10       	lea    0x10(%rsp),%rdi
   1a8d7:	31 f6                	xor    %esi,%esi
   1a8d9:	4c 89 ea             	mov    %r13,%rdx
   1a8dc:	e8 cf f6 04 00       	call   69fb0 <<alloc::raw_vec::RawVecInner<_>>::reserve::do_reserve_and_handle::<alloc::alloc::Global>>
   1a8e1:	48 8b 5c 24 20       	mov    0x20(%rsp),%rbx
   1a8e6:	e9 15 ff ff ff       	jmp    1a800 <cellbox::executor::terminal_read_line+0x940>
   1a8eb:	48 8b 44 24 08       	mov    0x8(%rsp),%rax
   1a8f0:	66 c7 40 08 0a 00    	movw   $0xa,0x8(%rax)
   1a8f6:	eb 0a                	jmp    1a902 <cellbox::executor::terminal_read_line+0xa42>
   1a8f8:	48 8b 44 24 08       	mov    0x8(%rsp),%rax
   1a8fd:	66 44 89 70 08       	mov    %r14w,0x8(%rax)
   1a902:	48 c7 00 fe ff ff ff 	movq   $0xfffffffffffffffe,(%rax)
   1a909:	c6 05 40 58 26 00 00 	movb   $0x0,0x265840(%rip)        # 280150 <cellbox::executor::SERIAL_SESSION>
   1a910:	eb 48                	jmp    1a95a <cellbox::executor::terminal_read_line+0xa9a>
   1a912:	48 8b 44 24 08       	mov    0x8(%rsp),%rax
   1a917:	48 c7 00 ff ff ff ff 	movq   $0xffffffffffffffff,(%rax)
   1a91e:	eb 2d                	jmp    1a94d <cellbox::executor::terminal_read_line+0xa8d>
   1a920:	48 8d 3d 97 91 21 00 	lea    0x219197(%rip),%rdi        # 233abe <unicode_width::tables::EMOJI_MODIFIER_LEAF_7+0x22ea>
   1a927:	be 03 00 00 00       	mov    $0x3,%esi
   1a92c:	31 d2                	xor    %edx,%edx
   1a92e:	e8 ad d2 ff ff       	call   17be0 <cellbox::executor::terminal_write>
   1a933:	66 83 f8 ff          	cmp    $0xffff,%ax
   1a937:	0f 84 6a 01 00 00    	je     1aaa7 <cellbox::executor::terminal_read_line+0xbe7>
   1a93d:	48 8b 4c 24 08       	mov    0x8(%rsp),%rcx
   1a942:	66 89 41 08          	mov    %ax,0x8(%rcx)
   1a946:	48 c7 01 fe ff ff ff 	movq   $0xfffffffffffffffe,(%rcx)
   1a94d:	48 8d bc 24 d0 00 00 	lea    0xd0(%rsp),%rdi
   1a954:	00 
   1a955:	e8 06 5f fe ff       	call   860 <core::ptr::drop_in_place::<cellbox::executor::ScrubbedEvent>>
   1a95a:	48 8b 54 24 30       	mov    0x30(%rsp),%rdx
   1a95f:	48 85 d2             	test   %rdx,%rdx
   1a962:	74 78                	je     1a9dc <cellbox::executor::terminal_read_line+0xb1c>
   1a964:	48 8b 44 24 38       	mov    0x38(%rsp),%rax
   1a969:	89 d1                	mov    %edx,%ecx
   1a96b:	83 e1 07             	and    $0x7,%ecx
   1a96e:	48 83 fa 08          	cmp    $0x8,%rdx
   1a972:	73 04                	jae    1a978 <cellbox::executor::terminal_read_line+0xab8>
   1a974:	31 f6                	xor    %esi,%esi
   1a976:	eb 4d                	jmp    1a9c5 <cellbox::executor::terminal_read_line+0xb05>
   1a978:	48 be f8 ff ff ff ff 	movabs $0x7ffffffffffffff8,%rsi
   1a97f:	ff ff 7f 
   1a982:	48 21 f2             	and    %rsi,%rdx
   1a985:	31 f6                	xor    %esi,%esi
   1a987:	66 0f 1f 84 00 00 00 	nopw   0x0(%rax,%rax,1)
   1a98e:	00 00 
   1a990:	c6 04 30 00          	movb   $0x0,(%rax,%rsi,1)
   1a994:	c6 44 30 01 00       	movb   $0x0,0x1(%rax,%rsi,1)
   1a999:	c6 44 30 02 00       	movb   $0x0,0x2(%rax,%rsi,1)
   1a99e:	c6 44 30 03 00       	movb   $0x0,0x3(%rax,%rsi,1)
   1a9a3:	c6 44 30 04 00       	movb   $0x0,0x4(%rax,%rsi,1)
   1a9a8:	c6 44 30 05 00       	movb   $0x0,0x5(%rax,%rsi,1)
   1a9ad:	c6 44 30 06 00       	movb   $0x0,0x6(%rax,%rsi,1)
   1a9b2:	c6 44 30 07 00       	movb   $0x0,0x7(%rax,%rsi,1)
   1a9b7:	48 83 c6 08          	add    $0x8,%rsi
   1a9bb:	48 39 f2             	cmp    %rsi,%rdx
   1a9be:	75 d0                	jne    1a990 <cellbox::executor::terminal_read_line+0xad0>
   1a9c0:	48 85 c9             	test   %rcx,%rcx
   1a9c3:	74 17                	je     1a9dc <cellbox::executor::terminal_read_line+0xb1c>
   1a9c5:	48 01 f0             	add    %rsi,%rax
   1a9c8:	31 d2                	xor    %edx,%edx
   1a9ca:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
   1a9d0:	c6 04 10 00          	movb   $0x0,(%rax,%rdx,1)
   1a9d4:	48 ff c2             	inc    %rdx
   1a9d7:	48 39 d1             	cmp    %rdx,%rcx
   1a9da:	75 f4                	jne    1a9d0 <cellbox::executor::terminal_read_line+0xb10>
   1a9dc:	48 8b 54 24 30       	mov    0x30(%rsp),%rdx
   1a9e1:	48 85 d2             	test   %rdx,%rdx
   1a9e4:	74 0f                	je     1a9f5 <cellbox::executor::terminal_read_line+0xb35>
   1a9e6:	48 8b 7c 24 38       	mov    0x38(%rsp),%rdi
   1a9eb:	be 01 00 00 00       	mov    $0x1,%esi
   1a9f0:	e8 2b 83 0c 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   1a9f5:	48 8b 54 24 10       	mov    0x10(%rsp),%rdx
   1a9fa:	48 85 d2             	test   %rdx,%rdx
   1a9fd:	74 7d                	je     1aa7c <cellbox::executor::terminal_read_line+0xbbc>
   1a9ff:	48 8b 44 24 18       	mov    0x18(%rsp),%rax
   1aa04:	89 d1                	mov    %edx,%ecx
   1aa06:	83 e1 07             	and    $0x7,%ecx
   1aa09:	48 83 fa 08          	cmp    $0x8,%rdx
   1aa0d:	73 04                	jae    1aa13 <cellbox::executor::terminal_read_line+0xb53>
   1aa0f:	31 f6                	xor    %esi,%esi
   1aa11:	eb 52                	jmp    1aa65 <cellbox::executor::terminal_read_line+0xba5>
   1aa13:	48 be f8 ff ff ff ff 	movabs $0x7ffffffffffffff8,%rsi
   1aa1a:	ff ff 7f 
   1aa1d:	48 21 f2             	and    %rsi,%rdx
   1aa20:	31 f6                	xor    %esi,%esi
   1aa22:	66 66 66 66 66 2e 0f 	data16 data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
   1aa29:	1f 84 00 00 00 00 00 
   1aa30:	c6 04 30 00          	movb   $0x0,(%rax,%rsi,1)
   1aa34:	c6 44 30 01 00       	movb   $0x0,0x1(%rax,%rsi,1)
   1aa39:	c6 44 30 02 00       	movb   $0x0,0x2(%rax,%rsi,1)
   1aa3e:	c6 44 30 03 00       	movb   $0x0,0x3(%rax,%rsi,1)
   1aa43:	c6 44 30 04 00       	movb   $0x0,0x4(%rax,%rsi,1)
   1aa48:	c6 44 30 05 00       	movb   $0x0,0x5(%rax,%rsi,1)
   1aa4d:	c6 44 30 06 00       	movb   $0x0,0x6(%rax,%rsi,1)
   1aa52:	c6 44 30 07 00       	movb   $0x0,0x7(%rax,%rsi,1)
   1aa57:	48 83 c6 08          	add    $0x8,%rsi
   1aa5b:	48 39 f2             	cmp    %rsi,%rdx
   1aa5e:	75 d0                	jne    1aa30 <cellbox::executor::terminal_read_line+0xb70>
   1aa60:	48 85 c9             	test   %rcx,%rcx
   1aa63:	74 17                	je     1aa7c <cellbox::executor::terminal_read_line+0xbbc>
   1aa65:	48 01 f0             	add    %rsi,%rax
   1aa68:	31 d2                	xor    %edx,%edx
   1aa6a:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
   1aa70:	c6 04 10 00          	movb   $0x0,(%rax,%rdx,1)
   1aa74:	48 ff c2             	inc    %rdx
   1aa77:	48 39 d1             	cmp    %rdx,%rcx
   1aa7a:	75 f4                	jne    1aa70 <cellbox::executor::terminal_read_line+0xbb0>
   1aa7c:	48 8b 54 24 10       	mov    0x10(%rsp),%rdx
   1aa81:	48 85 d2             	test   %rdx,%rdx
   1aa84:	74 0f                	je     1aa95 <cellbox::executor::terminal_read_line+0xbd5>
   1aa86:	48 8b 7c 24 18       	mov    0x18(%rsp),%rdi
   1aa8b:	be 01 00 00 00       	mov    $0x1,%esi
   1aa90:	e8 8b 82 0c 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   1aa95:	48 81 c4 88 01 00 00 	add    $0x188,%rsp
   1aa9c:	5b                   	pop    %rbx
   1aa9d:	41 5c                	pop    %r12
   1aa9f:	41 5d                	pop    %r13
   1aaa1:	41 5e                	pop    %r14
   1aaa3:	41 5f                	pop    %r15
   1aaa5:	5d                   	pop    %rbp
   1aaa6:	c3                   	ret
   1aaa7:	48 8b 44 24 08       	mov    0x8(%rsp),%rax
   1aaac:	48 c7 00 00 00 00 00 	movq   $0x0,(%rax)
   1aab3:	48 c7 40 08 01 00 00 	movq   $0x1,0x8(%rax)
   1aaba:	00 
   1aabb:	48 c7 40 10 00 00 00 	movq   $0x0,0x10(%rax)
   1aac2:	00 
   1aac3:	e9 85 fe ff ff       	jmp    1a94d <cellbox::executor::terminal_read_line+0xa8d>
   1aac8:	48 8d 3d 77 26 26 00 	lea    0x262677(%rip),%rdi        # 27d146 <core::unicode::unicode_data::white_space::WHITESPACE_MAP+0x27d>
   1aacf:	be 01 00 00 00       	mov    $0x1,%esi
   1aad4:	31 d2                	xor    %edx,%edx
   1aad6:	e8 05 d1 ff ff       	call   17be0 <cellbox::executor::terminal_write>
   1aadb:	66 83 f8 ff          	cmp    $0xffff,%ax
   1aadf:	0f 85 58 fe ff ff    	jne    1a93d <cellbox::executor::terminal_read_line+0xa7d>
   1aae5:	48 8b 5c 24 18       	mov    0x18(%rsp),%rbx
   1aaea:	4c 8b 74 24 20       	mov    0x20(%rsp),%r14
   1aaef:	48 89 df             	mov    %rbx,%rdi
   1aaf2:	4c 89 f6             	mov    %r14,%rsi
   1aaf5:	e8 76 15 1b 00       	call   1cc070 <<str>::trim_matches::<<char>::is_whitespace>>
   1aafa:	48 85 d2             	test   %rdx,%rdx
   1aafd:	4c 8b 64 24 08       	mov    0x8(%rsp),%r12
   1ab02:	0f 84 ca 00 00 00    	je     1abd2 <cellbox::executor::terminal_read_line+0xd12>
   1ab08:	48 8b 44 24 28       	mov    0x28(%rsp),%rax
   1ab0d:	4c 8b 78 10          	mov    0x10(%rax),%r15
   1ab11:	4d 85 ff             	test   %r15,%r15
   1ab14:	74 70                	je     1ab86 <cellbox::executor::terminal_read_line+0xcc6>
   1ab16:	48 8b 40 08          	mov    0x8(%rax),%rax
   1ab1a:	4b 8d 0c 7f          	lea    (%r15,%r15,2),%rcx
   1ab1e:	4c 39 74 c8 f8       	cmp    %r14,-0x8(%rax,%rcx,8)
   1ab23:	75 20                	jne    1ab45 <cellbox::executor::terminal_read_line+0xc85>
   1ab25:	48 8d 04 c8          	lea    (%rax,%rcx,8),%rax
   1ab29:	48 8b 40 f0          	mov    -0x10(%rax),%rax
   1ab2d:	31 c9                	xor    %ecx,%ecx
   1ab2f:	49 39 ce             	cmp    %rcx,%r14
   1ab32:	0f 84 9a 00 00 00    	je     1abd2 <cellbox::executor::terminal_read_line+0xd12>
   1ab38:	0f b6 14 08          	movzbl (%rax,%rcx,1),%edx
   1ab3c:	3a 14 0b             	cmp    (%rbx,%rcx,1),%dl
   1ab3f:	48 8d 49 01          	lea    0x1(%rcx),%rcx
   1ab43:	74 ea                	je     1ab2f <cellbox::executor::terminal_read_line+0xc6f>
   1ab45:	49 81 ff 80 00 00 00 	cmp    $0x80,%r15
   1ab4c:	75 3b                	jne    1ab89 <cellbox::executor::terminal_read_line+0xcc9>
   1ab4e:	48 8d 15 53 5a 26 00 	lea    0x265a53(%rip),%rdx        # 2805a8 <cellbox::executor::SSH_COMPLETION+0x88>
   1ab55:	48 8d 7c 24 70       	lea    0x70(%rsp),%rdi
   1ab5a:	4c 8b 7c 24 28       	mov    0x28(%rsp),%r15
   1ab5f:	4c 89 fe             	mov    %r15,%rsi
   1ab62:	e8 99 ff 01 00       	call   3ab00 <<alloc::vec::Vec<alloc::string::String>>::remove>
   1ab67:	48 8b 54 24 70       	mov    0x70(%rsp),%rdx
   1ab6c:	48 85 d2             	test   %rdx,%rdx
   1ab6f:	74 0f                	je     1ab80 <cellbox::executor::terminal_read_line+0xcc0>
   1ab71:	48 8b 7c 24 78       	mov    0x78(%rsp),%rdi
   1ab76:	be 01 00 00 00       	mov    $0x1,%esi
   1ab7b:	e8 a0 81 0c 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   1ab80:	4d 8b 7f 10          	mov    0x10(%r15),%r15
   1ab84:	eb 03                	jmp    1ab89 <cellbox::executor::terminal_read_line+0xcc9>
   1ab86:	45 31 ff             	xor    %r15d,%r15d
   1ab89:	48 8d 7c 24 70       	lea    0x70(%rsp),%rdi
   1ab8e:	48 89 de             	mov    %rbx,%rsi
   1ab91:	4c 89 f2             	mov    %r14,%rdx
   1ab94:	e8 27 b6 01 00       	call   361c0 <<alloc::string::String as core::clone::Clone>::clone>
   1ab99:	48 8b 5c 24 28       	mov    0x28(%rsp),%rbx
   1ab9e:	4c 3b 3b             	cmp    (%rbx),%r15
   1aba1:	74 7d                	je     1ac20 <cellbox::executor::terminal_read_line+0xd60>
   1aba3:	48 8b 43 08          	mov    0x8(%rbx),%rax
   1aba7:	4b 8d 0c 7f          	lea    (%r15,%r15,2),%rcx
   1abab:	48 8b 94 24 80 00 00 	mov    0x80(%rsp),%rdx
   1abb2:	00 
   1abb3:	48 89 54 c8 10       	mov    %rdx,0x10(%rax,%rcx,8)
   1abb8:	48 8b 54 24 70       	mov    0x70(%rsp),%rdx
   1abbd:	48 8b 74 24 78       	mov    0x78(%rsp),%rsi
   1abc2:	48 89 74 c8 08       	mov    %rsi,0x8(%rax,%rcx,8)
   1abc7:	48 89 14 c8          	mov    %rdx,(%rax,%rcx,8)
   1abcb:	49 ff c7             	inc    %r15
   1abce:	4c 89 7b 10          	mov    %r15,0x10(%rbx)
   1abd2:	48 8b 44 24 20       	mov    0x20(%rsp),%rax
   1abd7:	49 89 44 24 10       	mov    %rax,0x10(%r12)
   1abdc:	48 8b 44 24 10       	mov    0x10(%rsp),%rax
   1abe1:	48 8b 4c 24 18       	mov    0x18(%rsp),%rcx
   1abe6:	49 89 4c 24 08       	mov    %rcx,0x8(%r12)
   1abeb:	49 89 04 24          	mov    %rax,(%r12)
   1abef:	48 c7 44 24 10 00 00 	movq   $0x0,0x10(%rsp)
   1abf6:	00 00 
   1abf8:	48 c7 44 24 18 01 00 	movq   $0x1,0x18(%rsp)
   1abff:	00 00 
   1ac01:	48 c7 44 24 20 00 00 	movq   $0x0,0x20(%rsp)
   1ac08:	00 00 
   1ac0a:	e9 3e fd ff ff       	jmp    1a94d <cellbox::executor::terminal_read_line+0xa8d>
   1ac0f:	48 8d 35 ba a0 26 00 	lea    0x26a0ba(%rip),%rsi        # 284cd0 <cellbox::shell_state::ONESHOT+0x1250>
   1ac16:	bf 38 00 00 00       	mov    $0x38,%edi
   1ac1b:	e8 90 55 0b 00       	call   d01b0 <core::panicking::panic>
   1ac20:	48 89 df             	mov    %rbx,%rdi
   1ac23:	e8 08 9f 16 00       	call   184b30 <<alloc::raw_vec::RawVec<alloc::string::String>>::grow_one>
   1ac28:	e9 76 ff ff ff       	jmp    1aba3 <cellbox::executor::terminal_read_line+0xce3>
   1ac2d:	bf 01 00 00 00       	mov    $0x1,%edi
   1ac32:	be 00 10 00 00       	mov    $0x1000,%esi
   1ac37:	e8 3f b2 01 00       	call   35e7b <alloc::raw_vec::handle_error>
