
/tmp/cellbox-s10-3d-resume/cand/desktop.kernel.elf:     file format elf64-x86-64


Disassembly of section .text:

ffffffff80117200 <cellos_kernel::task::pipe::session_read>:
ffffffff80117200:	55                   	push   %rbp
ffffffff80117201:	41 57                	push   %r15
ffffffff80117203:	41 56                	push   %r14
ffffffff80117205:	41 55                	push   %r13
ffffffff80117207:	41 54                	push   %r12
ffffffff80117209:	53                   	push   %rbx
ffffffff8011720a:	48 83 ec 18          	sub    $0x18,%rsp
ffffffff8011720e:	48 89 f3             	mov    %rsi,%rbx
ffffffff80117211:	49 89 fe             	mov    %rdi,%r14
ffffffff80117214:	9c                   	pushf
ffffffff80117215:	41 5d                	pop    %r13
ffffffff80117217:	fa                   	cli
ffffffff80117218:	b1 01                	mov    $0x1,%cl
ffffffff8011721a:	31 c0                	xor    %eax,%eax
ffffffff8011721c:	f0 0f b0 0d 14 fb 97 	lock cmpxchg %cl,0x197fb14(%rip)        # ffffffff81a96d38 <cellos_kernel::task::pipe::PIPES+0x18>
ffffffff80117223:	01 
ffffffff80117224:	0f 85 08 01 00 00    	jne    ffffffff80117332 <cellos_kernel::task::pipe::session_read+0x132>
ffffffff8011722a:	48 8b 05 ef fa 97 01 	mov    0x197faef(%rip),%rax        # ffffffff81a96d20 <cellos_kernel::task::pipe::PIPES>
ffffffff80117231:	49 c7 c4 ff ff ff ff 	mov    $0xffffffffffffffff,%r12
ffffffff80117238:	48 85 c0             	test   %rax,%rax
ffffffff8011723b:	0f 84 88 01 00 00    	je     ffffffff801173c9 <cellos_kernel::task::pipe::session_read+0x1c9>
ffffffff80117241:	48 8b 0d e0 fa 97 01 	mov    0x197fae0(%rip),%rcx        # ffffffff81a96d28 <cellos_kernel::task::pipe::PIPES+0x8>
ffffffff80117248:	0f b7 b8 e2 05 00 00 	movzwl 0x5e2(%rax),%edi
ffffffff8011724f:	85 ff                	test   %edi,%edi
ffffffff80117251:	74 4d                	je     ffffffff801172a0 <cellos_kernel::task::pipe::session_read+0xa0>
ffffffff80117253:	44 8d 14 fd 00 00 00 	lea    0x0(,%rdi,8),%r10d
ffffffff8011725a:	00 
ffffffff8011725b:	31 f6                	xor    %esi,%esi
ffffffff8011725d:	0f 1f 00             	nopl   (%rax)
ffffffff80117260:	48 3b 94 f0 88 05 00 	cmp    0x588(%rax,%rsi,8),%rdx
ffffffff80117267:	00 
ffffffff80117268:	41 0f 97 c3          	seta   %r11b
ffffffff8011726c:	41 80 db 00          	sbb    $0x0,%r11b
ffffffff80117270:	41 80 fb 01          	cmp    $0x1,%r11b
ffffffff80117274:	75 1a                	jne    ffffffff80117290 <cellos_kernel::task::pipe::session_read+0x90>
ffffffff80117276:	48 ff c6             	inc    %rsi
ffffffff80117279:	49 83 c2 f8          	add    $0xfffffffffffffff8,%r10
ffffffff8011727d:	75 e1                	jne    ffffffff80117260 <cellos_kernel::task::pipe::session_read+0x60>
ffffffff8011727f:	48 89 fe             	mov    %rdi,%rsi
ffffffff80117282:	eb 1e                	jmp    ffffffff801172a2 <cellos_kernel::task::pipe::session_read+0xa2>
ffffffff80117284:	66 66 66 2e 0f 1f 84 	data16 data16 cs nopw 0x0(%rax,%rax,1)
ffffffff8011728b:	00 00 00 00 00 
ffffffff80117290:	41 0f b6 fb          	movzbl %r11b,%edi
ffffffff80117294:	85 ff                	test   %edi,%edi
ffffffff80117296:	74 1e                	je     ffffffff801172b6 <cellos_kernel::task::pipe::session_read+0xb6>
ffffffff80117298:	eb 08                	jmp    ffffffff801172a2 <cellos_kernel::task::pipe::session_read+0xa2>
ffffffff8011729a:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
ffffffff801172a0:	31 f6                	xor    %esi,%esi
ffffffff801172a2:	48 83 e9 01          	sub    $0x1,%rcx
ffffffff801172a6:	0f 82 1d 01 00 00    	jb     ffffffff801173c9 <cellos_kernel::task::pipe::session_read+0x1c9>
ffffffff801172ac:	48 8b 84 f0 e8 05 00 	mov    0x5e8(%rax,%rsi,8),%rax
ffffffff801172b3:	00 
ffffffff801172b4:	eb 92                	jmp    ffffffff80117248 <cellos_kernel::task::pipe::session_read+0x48>
ffffffff801172b6:	48 c1 e6 07          	shl    $0x7,%rsi
ffffffff801172ba:	48 8d 2c 30          	lea    (%rax,%rsi,1),%rbp
ffffffff801172be:	48 8b 74 30 60       	mov    0x60(%rax,%rsi,1),%rsi
ffffffff801172c3:	48 85 f6             	test   %rsi,%rsi
ffffffff801172c6:	0f 84 ee 00 00 00    	je     ffffffff801173ba <cellos_kernel::task::pipe::session_read+0x1ba>
ffffffff801172cc:	4c 8b 55 08          	mov    0x8(%rbp),%r10
ffffffff801172d0:	48 8b 7d 10          	mov    0x10(%rbp),%rdi
ffffffff801172d4:	49 39 f1             	cmp    %rsi,%r9
ffffffff801172d7:	49 89 f7             	mov    %rsi,%r15
ffffffff801172da:	4d 0f 42 f9          	cmovb  %r9,%r15
ffffffff801172de:	48 8b 4d 58          	mov    0x58(%rbp),%rcx
ffffffff801172e2:	45 31 db             	xor    %r11d,%r11d
ffffffff801172e5:	eb 23                	jmp    ffffffff8011730a <cellos_kernel::task::pipe::session_read+0x10a>
ffffffff801172e7:	66 0f 1f 84 00 00 00 	nopw   0x0(%rax,%rax,1)
ffffffff801172ee:	00 00 
ffffffff801172f0:	31 d2                	xor    %edx,%edx
ffffffff801172f2:	48 f7 f7             	div    %rdi
ffffffff801172f5:	41 0f b6 04 12       	movzbl (%r10,%rdx,1),%eax
ffffffff801172fa:	43 88 04 18          	mov    %al,(%r8,%r11,1)
ffffffff801172fe:	49 ff c3             	inc    %r11
ffffffff80117301:	4d 39 df             	cmp    %r11,%r15
ffffffff80117304:	0f 84 ee 00 00 00    	je     ffffffff801173f8 <cellos_kernel::task::pipe::session_read+0x1f8>
ffffffff8011730a:	4d 39 d9             	cmp    %r11,%r9
ffffffff8011730d:	0f 84 dc 00 00 00    	je     ffffffff801173ef <cellos_kernel::task::pipe::session_read+0x1ef>
ffffffff80117313:	48 85 ff             	test   %rdi,%rdi
ffffffff80117316:	0f 84 2a 02 00 00    	je     ffffffff80117546 <cellos_kernel::task::pipe::session_read+0x346>
ffffffff8011731c:	4a 8d 04 19          	lea    (%rcx,%r11,1),%rax
ffffffff80117320:	48 89 c2             	mov    %rax,%rdx
ffffffff80117323:	48 09 fa             	or     %rdi,%rdx
ffffffff80117326:	48 c1 ea 20          	shr    $0x20,%rdx
ffffffff8011732a:	75 c4                	jne    ffffffff801172f0 <cellos_kernel::task::pipe::session_read+0xf0>
ffffffff8011732c:	31 d2                	xor    %edx,%edx
ffffffff8011732e:	f7 f7                	div    %edi
ffffffff80117330:	eb c3                	jmp    ffffffff801172f5 <cellos_kernel::task::pipe::session_read+0xf5>
ffffffff80117332:	be 02 00 00 00       	mov    $0x2,%esi
ffffffff80117337:	48 8d 7c 24 08       	lea    0x8(%rsp),%rdi
ffffffff8011733c:	eb 31                	jmp    ffffffff8011736f <cellos_kernel::task::pipe::session_read+0x16f>
ffffffff8011733e:	48 c7 44 24 10 00 00 	movq   $0x0,0x10(%rsp)
ffffffff80117345:	00 00 
ffffffff80117347:	48 c7 44 24 08 00 00 	movq   $0x0,0x8(%rsp)
ffffffff8011734e:	00 00 
ffffffff80117350:	66 0f 38 82 37       	invpcid (%rdi),%rsi
ffffffff80117355:	4e 89 1c d5 50 3e a9 	mov    %r11,-0x7e56c1b0(,%r10,8)
ffffffff8011735c:	81 
ffffffff8011735d:	f3 90                	pause
ffffffff8011735f:	31 c0                	xor    %eax,%eax
ffffffff80117361:	f0 0f b0 0d cf f9 97 	lock cmpxchg %cl,0x197f9cf(%rip)        # ffffffff81a96d38 <cellos_kernel::task::pipe::PIPES+0x18>
ffffffff80117368:	01 
ffffffff80117369:	0f 84 bb fe ff ff    	je     ffffffff8011722a <cellos_kernel::task::pipe::session_read+0x2a>
ffffffff8011736f:	65 4c 8b 14 25 20 00 	mov    %gs:0x20,%r10
ffffffff80117376:	00 00 
ffffffff80117378:	49 83 fa 01          	cmp    $0x1,%r10
ffffffff8011737c:	77 df                	ja     ffffffff8011735d <cellos_kernel::task::pipe::session_read+0x15d>
ffffffff8011737e:	4e 8b 1c d5 38 3e a9 	mov    -0x7e56c1c8(,%r10,8),%r11
ffffffff80117385:	81 
ffffffff80117386:	4a 8b 04 d5 50 3e a9 	mov    -0x7e56c1b0(,%r10,8),%rax
ffffffff8011738d:	81 
ffffffff8011738e:	4c 39 d8             	cmp    %r11,%rax
ffffffff80117391:	73 ca                	jae    ffffffff8011735d <cellos_kernel::task::pipe::session_read+0x15d>
ffffffff80117393:	0f b6 05 de 1b 98 01 	movzbl 0x1981bde(%rip),%eax        # ffffffff81a98f78 <hal_x86::x86_64::domain::PCID_USABLE>
ffffffff8011739a:	84 c0                	test   %al,%al
ffffffff8011739c:	75 a0                	jne    ffffffff8011733e <cellos_kernel::task::pipe::session_read+0x13e>
ffffffff8011739e:	0f 20 e0             	mov    %cr4,%rax
ffffffff801173a1:	49 89 c7             	mov    %rax,%r15
ffffffff801173a4:	49 81 f7 80 00 00 00 	xor    $0x80,%r15
ffffffff801173ab:	41 0f 22 e7          	mov    %r15,%cr4
ffffffff801173af:	0f 22 e0             	mov    %rax,%cr4
ffffffff801173b2:	0f 20 d8             	mov    %cr3,%rax
ffffffff801173b5:	0f 22 d8             	mov    %rax,%cr3
ffffffff801173b8:	eb 9b                	jmp    ffffffff80117355 <cellos_kernel::task::pipe::session_read+0x155>
ffffffff801173ba:	48 83 7d 70 01       	cmpq   $0x1,0x70(%rbp)
ffffffff801173bf:	41 bc 02 00 00 00    	mov    $0x2,%r12d
ffffffff801173c5:	49 83 dc 00          	sbb    $0x0,%r12
ffffffff801173c9:	c6 05 68 f9 97 01 00 	movb   $0x0,0x197f968(%rip)        # ffffffff81a96d38 <cellos_kernel::task::pipe::PIPES+0x18>
ffffffff801173d0:	41 f7 c5 00 02 00 00 	test   $0x200,%r13d
ffffffff801173d7:	74 01                	je     ffffffff801173da <cellos_kernel::task::pipe::session_read+0x1da>
ffffffff801173d9:	fb                   	sti
ffffffff801173da:	4c 89 e0             	mov    %r12,%rax
ffffffff801173dd:	4c 89 fa             	mov    %r15,%rdx
ffffffff801173e0:	48 83 c4 18          	add    $0x18,%rsp
ffffffff801173e4:	5b                   	pop    %rbx
ffffffff801173e5:	41 5c                	pop    %r12
ffffffff801173e7:	41 5d                	pop    %r13
ffffffff801173e9:	41 5e                	pop    %r14
ffffffff801173eb:	41 5f                	pop    %r15
ffffffff801173ed:	5d                   	pop    %rbp
ffffffff801173ee:	c3                   	ret
ffffffff801173ef:	48 85 ff             	test   %rdi,%rdi
ffffffff801173f2:	0f 84 5a 01 00 00    	je     ffffffff80117552 <cellos_kernel::task::pipe::session_read+0x352>
ffffffff801173f8:	4c 01 f9             	add    %r15,%rcx
ffffffff801173fb:	48 89 c8             	mov    %rcx,%rax
ffffffff801173fe:	48 09 f8             	or     %rdi,%rax
ffffffff80117401:	48 c1 e8 20          	shr    $0x20,%rax
ffffffff80117405:	74 0a                	je     ffffffff80117411 <cellos_kernel::task::pipe::session_read+0x211>
ffffffff80117407:	48 89 c8             	mov    %rcx,%rax
ffffffff8011740a:	31 d2                	xor    %edx,%edx
ffffffff8011740c:	48 f7 f7             	div    %rdi
ffffffff8011740f:	eb 06                	jmp    ffffffff80117417 <cellos_kernel::task::pipe::session_read+0x217>
ffffffff80117411:	89 c8                	mov    %ecx,%eax
ffffffff80117413:	31 d2                	xor    %edx,%edx
ffffffff80117415:	f7 f7                	div    %edi
ffffffff80117417:	48 89 55 58          	mov    %rdx,0x58(%rbp)
ffffffff8011741b:	4c 29 fe             	sub    %r15,%rsi
ffffffff8011741e:	48 89 75 60          	mov    %rsi,0x60(%rbp)
ffffffff80117422:	45 31 e4             	xor    %r12d,%r12d
ffffffff80117425:	66 66 2e 0f 1f 84 00 	data16 cs nopw 0x0(%rax,%rax,1)
ffffffff8011742c:	00 00 00 00 
ffffffff80117430:	48 8b 4d 50          	mov    0x50(%rbp),%rcx
ffffffff80117434:	48 85 c9             	test   %rcx,%rcx
ffffffff80117437:	74 90                	je     ffffffff801173c9 <cellos_kernel::task::pipe::session_read+0x1c9>
ffffffff80117439:	48 8b 45 48          	mov    0x48(%rbp),%rax
ffffffff8011743d:	48 8d 70 01          	lea    0x1(%rax),%rsi
ffffffff80117441:	48 8b 7d 38          	mov    0x38(%rbp),%rdi
ffffffff80117445:	48 8b 55 40          	mov    0x40(%rbp),%rdx
ffffffff80117449:	48 39 fe             	cmp    %rdi,%rsi
ffffffff8011744c:	49 0f 42 fc          	cmovb  %r12,%rdi
ffffffff80117450:	48 f7 df             	neg    %rdi
ffffffff80117453:	48 8d 34 38          	lea    (%rax,%rdi,1),%rsi
ffffffff80117457:	48 ff c6             	inc    %rsi
ffffffff8011745a:	48 89 75 48          	mov    %rsi,0x48(%rbp)
ffffffff8011745e:	48 ff c9             	dec    %rcx
ffffffff80117461:	48 89 4d 50          	mov    %rcx,0x50(%rbp)
ffffffff80117465:	48 85 db             	test   %rbx,%rbx
ffffffff80117468:	74 c6                	je     ffffffff80117430 <cellos_kernel::task::pipe::session_read+0x230>
ffffffff8011746a:	48 8b 34 c2          	mov    (%rdx,%rax,8),%rsi
ffffffff8011746e:	b8 00 00 00 00       	mov    $0x0,%eax
ffffffff80117473:	48 83 fb 01          	cmp    $0x1,%rbx
ffffffff80117477:	74 2b                	je     ffffffff801174a4 <cellos_kernel::task::pipe::session_read+0x2a4>
ffffffff80117479:	48 89 d9             	mov    %rbx,%rcx
ffffffff8011747c:	31 c0                	xor    %eax,%eax
ffffffff8011747e:	66 90                	xchg   %ax,%ax
ffffffff80117480:	48 89 c2             	mov    %rax,%rdx
ffffffff80117483:	48 89 cf             	mov    %rcx,%rdi
ffffffff80117486:	48 d1 ef             	shr    $1,%rdi
ffffffff80117489:	48 01 f8             	add    %rdi,%rax
ffffffff8011748c:	49 89 c0             	mov    %rax,%r8
ffffffff8011748f:	49 c1 e0 04          	shl    $0x4,%r8
ffffffff80117493:	4b 39 34 06          	cmp    %rsi,(%r14,%r8,1)
ffffffff80117497:	48 0f 47 c2          	cmova  %rdx,%rax
ffffffff8011749b:	48 29 f9             	sub    %rdi,%rcx
ffffffff8011749e:	48 83 f9 01          	cmp    $0x1,%rcx
ffffffff801174a2:	77 dc                	ja     ffffffff80117480 <cellos_kernel::task::pipe::session_read+0x280>
ffffffff801174a4:	48 c1 e0 04          	shl    $0x4,%rax
ffffffff801174a8:	49 39 34 06          	cmp    %rsi,(%r14,%rax,1)
ffffffff801174ac:	75 82                	jne    ffffffff80117430 <cellos_kernel::task::pipe::session_read+0x230>
ffffffff801174ae:	4c 01 f0             	add    %r14,%rax
ffffffff801174b1:	48 8b 40 08          	mov    0x8(%rax),%rax
ffffffff801174b5:	0f b6 48 60          	movzbl 0x60(%rax),%ecx
ffffffff801174b9:	80 c1 f9             	add    $0xf9,%cl
ffffffff801174bc:	80 f9 01             	cmp    $0x1,%cl
ffffffff801174bf:	0f 87 6b ff ff ff    	ja     ffffffff80117430 <cellos_kernel::task::pipe::session_read+0x230>
ffffffff801174c5:	c6 40 60 00          	movb   $0x0,0x60(%rax)
ffffffff801174c9:	b0 01                	mov    $0x1,%al
ffffffff801174cb:	48 85 db             	test   %rbx,%rbx
ffffffff801174ce:	74 60                	je     ffffffff80117530 <cellos_kernel::task::pipe::session_read+0x330>
ffffffff801174d0:	48 83 fb 01          	cmp    $0x1,%rbx
ffffffff801174d4:	75 04                	jne    ffffffff801174da <cellos_kernel::task::pipe::session_read+0x2da>
ffffffff801174d6:	31 c9                	xor    %ecx,%ecx
ffffffff801174d8:	eb 2a                	jmp    ffffffff80117504 <cellos_kernel::task::pipe::session_read+0x304>
ffffffff801174da:	48 89 da             	mov    %rbx,%rdx
ffffffff801174dd:	31 c9                	xor    %ecx,%ecx
ffffffff801174df:	90                   	nop
ffffffff801174e0:	48 89 cf             	mov    %rcx,%rdi
ffffffff801174e3:	49 89 d0             	mov    %rdx,%r8
ffffffff801174e6:	49 d1 e8             	shr    $1,%r8
ffffffff801174e9:	4c 01 c1             	add    %r8,%rcx
ffffffff801174ec:	49 89 c9             	mov    %rcx,%r9
ffffffff801174ef:	49 c1 e1 04          	shl    $0x4,%r9
ffffffff801174f3:	4b 39 34 0e          	cmp    %rsi,(%r14,%r9,1)
ffffffff801174f7:	48 0f 47 cf          	cmova  %rdi,%rcx
ffffffff801174fb:	4c 29 c2             	sub    %r8,%rdx
ffffffff801174fe:	48 83 fa 01          	cmp    $0x1,%rdx
ffffffff80117502:	77 dc                	ja     ffffffff801174e0 <cellos_kernel::task::pipe::session_read+0x2e0>
ffffffff80117504:	48 c1 e1 04          	shl    $0x4,%rcx
ffffffff80117508:	49 39 34 0e          	cmp    %rsi,(%r14,%rcx,1)
ffffffff8011750c:	75 22                	jne    ffffffff80117530 <cellos_kernel::task::pipe::session_read+0x330>
ffffffff8011750e:	4c 01 f1             	add    %r14,%rcx
ffffffff80117511:	48 8b 41 08          	mov    0x8(%rcx),%rax
ffffffff80117515:	0f b6 80 d9 05 00 00 	movzbl 0x5d9(%rax),%eax
ffffffff8011751c:	3c 02                	cmp    $0x2,%al
ffffffff8011751e:	72 10                	jb     ffffffff80117530 <cellos_kernel::task::pipe::session_read+0x330>
ffffffff80117520:	0f b6 0d 32 ec 97 01 	movzbl 0x197ec32(%rip),%ecx        # ffffffff81a96159 <cellos_kernel::task::smp::HART_ONLINE+0x1>
ffffffff80117527:	bf 01 00 00 00       	mov    $0x1,%edi
ffffffff8011752c:	84 c9                	test   %cl,%cl
ffffffff8011752e:	75 09                	jne    ffffffff80117539 <cellos_kernel::task::pipe::session_read+0x339>
ffffffff80117530:	65 48 8b 3c 25 20 00 	mov    %gs:0x20,%rdi
ffffffff80117537:	00 00 
ffffffff80117539:	0f b6 d0             	movzbl %al,%edx
ffffffff8011753c:	e8 ef 8d f8 ff       	call   ffffffff800a0330 <cellos_kernel::task::hart_local::ready::push_on_hart>
ffffffff80117541:	e9 ea fe ff ff       	jmp    ffffffff80117430 <cellos_kernel::task::pipe::session_read+0x230>
ffffffff80117546:	48 c7 c7 68 df 9f 81 	mov    $0xffffffff819fdf68,%rdi
ffffffff8011754d:	e8 4e 3e 01 00       	call   ffffffff8012b3a0 <core::panicking::panic_const::panic_const_rem_by_zero>
ffffffff80117552:	48 c7 c7 80 df 9f 81 	mov    $0xffffffff819fdf80,%rdi
ffffffff80117559:	e8 42 3e 01 00       	call   ffffffff8012b3a0 <core::panicking::panic_const::panic_const_rem_by_zero>
