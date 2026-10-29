
/tmp/cellbox-s10-3d-resume/cand/desktop/files/terminal:     file format elf64-x86-64


Disassembly of section .text:

0000000000000040 <terminal::session::drain::<terminal_sdk::host::FrontendHost>>:
      40:	55                   	push   %rbp
      41:	41 57                	push   %r15
      43:	41 56                	push   %r14
      45:	41 55                	push   %r13
      47:	41 54                	push   %r12
      49:	53                   	push   %rbx
      4a:	48 81 ec 18 02 00 00 	sub    $0x218,%rsp
      51:	48 89 d3             	mov    %rdx,%rbx
      54:	49 89 f6             	mov    %rsi,%r14
      57:	49 89 ff             	mov    %rdi,%r15
      5a:	48 8d 7c 24 18       	lea    0x18(%rsp),%rdi
      5f:	ba 00 02 00 00       	mov    $0x200,%edx
      64:	31 f6                	xor    %esi,%esi
      66:	ff 15 1c cd 0d 00    	call   *0xdcd1c(%rip)        # dcd88 <ostd::startup::_KEEP_START+0x6b0>
      6c:	66 b8 11 00          	mov    $0x11,%ax
      70:	41 80 bf 4c 01 00 00 	cmpb   $0x0,0x14c(%r15)
      77:	00 
      78:	74 23                	je     9d <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x5d>
      7a:	66 89 44 24 0a       	mov    %ax,0xa(%rsp)
      7f:	0f b7 6c 24 0a       	movzwl 0xa(%rsp),%ebp
      84:	c1 e5 10             	shl    $0x10,%ebp
      87:	ff c5                	inc    %ebp
      89:	89 e8                	mov    %ebp,%eax
      8b:	48 81 c4 18 02 00 00 	add    $0x218,%rsp
      92:	5b                   	pop    %rbx
      93:	41 5c                	pop    %r12
      95:	41 5d                	pop    %r13
      97:	41 5e                	pop    %r14
      99:	41 5f                	pop    %r15
      9b:	5d                   	pop    %rbp
      9c:	c3                   	ret
      9d:	49 8d b7 c8 00 00 00 	lea    0xc8(%r15),%rsi
      a4:	48 8d 7c 24 08       	lea    0x8(%rsp),%rdi
      a9:	48 8d 54 24 18       	lea    0x18(%rsp),%rdx
      ae:	e8 ad 6d 02 00       	call   26e60 <<terminal_sdk::stream::Endpoint>::read>
      b3:	66 83 7c 24 08 01    	cmpw   $0x1,0x8(%rsp)
      b9:	75 62                	jne    11d <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0xdd>
      bb:	0f b7 44 24 0a       	movzwl 0xa(%rsp),%eax
      c0:	83 f8 0f             	cmp    $0xf,%eax
      c3:	77 ba                	ja     7f <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x3f>
      c5:	b9 02 c0 00 00       	mov    $0xc002,%ecx
      ca:	0f a3 c1             	bt     %eax,%ecx
      cd:	73 b0                	jae    7f <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x3f>
      cf:	31 ed                	xor    %ebp,%ebp
      d1:	49 81 c7 e8 00 00 00 	add    $0xe8,%r15
      d8:	48 8d 7c 24 08       	lea    0x8(%rsp),%rdi
      dd:	48 8d 54 24 18       	lea    0x18(%rsp),%rdx
      e2:	4c 89 fe             	mov    %r15,%rsi
      e5:	e8 76 6d 02 00       	call   26e60 <<terminal_sdk::stream::Endpoint>::read>
      ea:	66 83 7c 24 08 01    	cmpw   $0x1,0x8(%rsp)
      f0:	0f 85 b9 00 00 00    	jne    1af <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x16f>
      f6:	0f b7 44 24 0a       	movzwl 0xa(%rsp),%eax
      fb:	83 f8 0f             	cmp    $0xf,%eax
      fe:	0f 87 7b ff ff ff    	ja     7f <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x3f>
     104:	b9 02 c0 00 00       	mov    $0xc002,%ecx
     109:	0f a3 c1             	bt     %eax,%ecx
     10c:	0f 83 6d ff ff ff    	jae    7f <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x3f>
     112:	81 e5 00 01 00 00    	and    $0x100,%ebp
     118:	e9 6c ff ff ff       	jmp    89 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x49>
     11d:	48 8b 4c 24 10       	mov    0x10(%rsp),%rcx
     122:	66 b8 03 00          	mov    $0x3,%ax
     126:	48 81 f9 00 02 00 00 	cmp    $0x200,%rcx
     12d:	0f 87 47 ff ff ff    	ja     7a <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x3a>
     133:	48 85 c9             	test   %rcx,%rcx
     136:	74 97                	je     cf <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x8f>
     138:	4c 8d 6c 24 18       	lea    0x18(%rsp),%r13
     13d:	4c 89 f7             	mov    %r14,%rdi
     140:	48 89 de             	mov    %rbx,%rsi
     143:	4c 89 ea             	mov    %r13,%rdx
     146:	e8 35 d9 01 00       	call   1da80 <<terminal::sink::ByteSink>::feed>
     14b:	41 bc 1f 00 00 00    	mov    $0x1f,%r12d
     151:	bd 00 01 00 00       	mov    $0x100,%ebp
     156:	66 2e 0f 1f 84 00 00 	cs nopw 0x0(%rax,%rax,1)
     15d:	00 00 00 
     160:	48 8d 7c 24 08       	lea    0x8(%rsp),%rdi
     165:	49 8d b7 c8 00 00 00 	lea    0xc8(%r15),%rsi
     16c:	4c 89 ea             	mov    %r13,%rdx
     16f:	e8 ec 6c 02 00       	call   26e60 <<terminal_sdk::stream::Endpoint>::read>
     174:	66 83 7c 24 08 01    	cmpw   $0x1,0x8(%rsp)
     17a:	0f 84 b7 00 00 00    	je     237 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x1f7>
     180:	48 8b 4c 24 10       	mov    0x10(%rsp),%rcx
     185:	48 81 f9 00 02 00 00 	cmp    $0x200,%rcx
     18c:	77 2f                	ja     1bd <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x17d>
     18e:	48 85 c9             	test   %rcx,%rcx
     191:	0f 84 3a ff ff ff    	je     d1 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x91>
     197:	4c 89 f7             	mov    %r14,%rdi
     19a:	48 89 de             	mov    %rbx,%rsi
     19d:	4c 89 ea             	mov    %r13,%rdx
     1a0:	e8 db d8 01 00       	call   1da80 <<terminal::sink::ByteSink>::feed>
     1a5:	49 ff cc             	dec    %r12
     1a8:	75 b6                	jne    160 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x120>
     1aa:	e9 22 ff ff ff       	jmp    d1 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x91>
     1af:	48 8b 4c 24 10       	mov    0x10(%rsp),%rcx
     1b4:	48 81 f9 00 02 00 00 	cmp    $0x200,%rcx
     1bb:	76 0c                	jbe    1c9 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x189>
     1bd:	66 c7 44 24 0a 03 00 	movw   $0x3,0xa(%rsp)
     1c4:	e9 b6 fe ff ff       	jmp    7f <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x3f>
     1c9:	48 85 c9             	test   %rcx,%rcx
     1cc:	0f 84 40 ff ff ff    	je     112 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0xd2>
     1d2:	4c 8d 64 24 18       	lea    0x18(%rsp),%r12
     1d7:	4c 89 f7             	mov    %r14,%rdi
     1da:	48 89 de             	mov    %rbx,%rsi
     1dd:	4c 89 e2             	mov    %r12,%rdx
     1e0:	e8 9b d8 01 00       	call   1da80 <<terminal::sink::ByteSink>::feed>
     1e5:	41 bd 1f 00 00 00    	mov    $0x1f,%r13d
     1eb:	bd 00 01 00 00       	mov    $0x100,%ebp
     1f0:	48 8d 7c 24 08       	lea    0x8(%rsp),%rdi
     1f5:	4c 89 fe             	mov    %r15,%rsi
     1f8:	4c 89 e2             	mov    %r12,%rdx
     1fb:	e8 60 6c 02 00       	call   26e60 <<terminal_sdk::stream::Endpoint>::read>
     200:	66 83 7c 24 08 01    	cmpw   $0x1,0x8(%rsp)
     206:	74 55                	je     25d <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x21d>
     208:	48 8b 4c 24 10       	mov    0x10(%rsp),%rcx
     20d:	48 81 f9 00 02 00 00 	cmp    $0x200,%rcx
     214:	77 a7                	ja     1bd <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x17d>
     216:	48 85 c9             	test   %rcx,%rcx
     219:	0f 84 f3 fe ff ff    	je     112 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0xd2>
     21f:	4c 89 f7             	mov    %r14,%rdi
     222:	48 89 de             	mov    %rbx,%rsi
     225:	4c 89 e2             	mov    %r12,%rdx
     228:	e8 53 d8 01 00       	call   1da80 <<terminal::sink::ByteSink>::feed>
     22d:	49 ff cd             	dec    %r13
     230:	75 be                	jne    1f0 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x1b0>
     232:	e9 db fe ff ff       	jmp    112 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0xd2>
     237:	0f b7 44 24 0a       	movzwl 0xa(%rsp),%eax
     23c:	83 f8 0f             	cmp    $0xf,%eax
     23f:	0f 87 3a fe ff ff    	ja     7f <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x3f>
     245:	b9 02 c0 00 00       	mov    $0xc002,%ecx
     24a:	0f a3 c1             	bt     %eax,%ecx
     24d:	0f 83 2c fe ff ff    	jae    7f <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x3f>
     253:	bd 00 01 00 00       	mov    $0x100,%ebp
     258:	e9 74 fe ff ff       	jmp    d1 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x91>
     25d:	0f b7 44 24 0a       	movzwl 0xa(%rsp),%eax
     262:	83 f8 0f             	cmp    $0xf,%eax
     265:	0f 87 14 fe ff ff    	ja     7f <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x3f>
     26b:	b9 02 c0 00 00       	mov    $0xc002,%ecx
     270:	0f a3 c1             	bt     %eax,%ecx
     273:	0f 83 06 fe ff ff    	jae    7f <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0x3f>
     279:	bd 00 01 00 00       	mov    $0x100,%ebp
     27e:	e9 8f fe ff ff       	jmp    112 <terminal::session::drain::<terminal_sdk::host::FrontendHost>+0xd2>
     283:	cc                   	int3
     284:	cc                   	int3
     285:	cc                   	int3
     286:	cc                   	int3
     287:	cc                   	int3
     288:	cc                   	int3
     289:	cc                   	int3
     28a:	cc                   	int3
     28b:	cc                   	int3
     28c:	cc                   	int3
     28d:	cc                   	int3
     28e:	cc                   	int3
     28f:	cc                   	int3

0000000000000290 <<alloc::raw_vec::RawVecInner<_>>::reserve::do_reserve_and_handle::<alloc::alloc::Global>>:
     290:	41 56                	push   %r14
     292:	53                   	push   %rbx
     293:	48 83 ec 18          	sub    $0x18,%rsp
     297:	48 01 d6             	add    %rdx,%rsi
     29a:	72 57                	jb     2f3 <<alloc::raw_vec::RawVecInner<_>>::reserve::do_reserve_and_handle::<alloc::alloc::Global>+0x63>
     29c:	48 89 fb             	mov    %rdi,%rbx
     29f:	48 8b 07             	mov    (%rdi),%rax
     2a2:	48 8d 0c 00          	lea    (%rax,%rax,1),%rcx
     2a6:	48 39 ce             	cmp    %rcx,%rsi
     2a9:	48 0f 47 ce          	cmova  %rsi,%rcx
     2ad:	48 83 f9 05          	cmp    $0x5,%rcx
     2b1:	41 be 04 00 00 00    	mov    $0x4,%r14d
     2b7:	4c 0f 43 f1          	cmovae %rcx,%r14
     2bb:	48 8b 57 08          	mov    0x8(%rdi),%rdx
     2bf:	48 89 e7             	mov    %rsp,%rdi
     2c2:	41 b8 04 00 00 00    	mov    $0x4,%r8d
     2c8:	41 b9 14 00 00 00    	mov    $0x14,%r9d
     2ce:	48 89 c6             	mov    %rax,%rsi
     2d1:	4c 89 f1             	mov    %r14,%rcx
     2d4:	e8 e7 18 00 00       	call   1bc0 <<alloc::raw_vec::RawVecInner>::finish_grow>
     2d9:	83 3c 24 01          	cmpl   $0x1,(%rsp)
     2dd:	74 1b                	je     2fa <<alloc::raw_vec::RawVecInner<_>>::reserve::do_reserve_and_handle::<alloc::alloc::Global>+0x6a>
     2df:	48 8b 44 24 08       	mov    0x8(%rsp),%rax
     2e4:	48 89 43 08          	mov    %rax,0x8(%rbx)
     2e8:	4c 89 33             	mov    %r14,(%rbx)
     2eb:	48 83 c4 18          	add    $0x18,%rsp
     2ef:	5b                   	pop    %rbx
     2f0:	41 5e                	pop    %r14
     2f2:	c3                   	ret
     2f3:	31 ff                	xor    %edi,%edi
     2f5:	e8 e2 72 00 00       	call   75dc <alloc::raw_vec::handle_error>
     2fa:	48 8b 7c 24 08       	mov    0x8(%rsp),%rdi
     2ff:	48 8b 74 24 10       	mov    0x10(%rsp),%rsi
     304:	e8 d3 72 00 00       	call   75dc <alloc::raw_vec::handle_error>
     309:	cc                   	int3
     30a:	cc                   	int3
     30b:	cc                   	int3
     30c:	cc                   	int3
     30d:	cc                   	int3
     30e:	cc                   	int3
     30f:	cc                   	int3
