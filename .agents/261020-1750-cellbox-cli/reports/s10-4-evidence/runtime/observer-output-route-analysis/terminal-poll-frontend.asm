
/tmp/cellbox-s10-3d-resume/cand/desktop/files/terminal:     file format elf64-x86-64


Disassembly of section .text:

0000000000026290 <<terminal_sdk::host::FrontendHost>::poll_frontend>:
   26290:	53                   	push   %rbx
   26291:	48 81 ec b0 00 00 00 	sub    $0xb0,%rsp
   26298:	b8 01 00 11 00       	mov    $0x110001,%eax
   2629d:	80 bf 4c 01 00 00 00 	cmpb   $0x0,0x14c(%rdi)
   262a4:	0f 85 59 01 00 00    	jne    26403 <<terminal_sdk::host::FrontendHost>::poll_frontend+0x173>
   262aa:	b8 00 03 00 00       	mov    $0x300,%eax
   262af:	80 bf 4d 01 00 00 03 	cmpb   $0x3,0x14d(%rdi)
   262b6:	0f 84 47 01 00 00    	je     26403 <<terminal_sdk::host::FrontendHost>::poll_frontend+0x173>
   262bc:	b8 01 00 07 00       	mov    $0x70001,%eax
   262c1:	f6 07 01             	testb  $0x1,(%rdi)
   262c4:	0f 84 39 01 00 00    	je     26403 <<terminal_sdk::host::FrontendHost>::poll_frontend+0x173>
   262ca:	48 8b 47 08          	mov    0x8(%rdi),%rax
   262ce:	48 8b 4f 10          	mov    0x10(%rdi),%rcx
   262d2:	48 c7 84 24 a8 00 00 	movq   $0x0,0xa8(%rsp)
   262d9:	00 00 00 00 00 
   262de:	48 c7 84 24 a0 00 00 	movq   $0x0,0xa0(%rsp)
   262e5:	00 00 00 00 00 
   262ea:	48 c7 84 24 98 00 00 	movq   $0x0,0x98(%rsp)
   262f1:	00 00 00 00 00 
   262f6:	48 c7 84 24 90 00 00 	movq   $0x0,0x90(%rsp)
   262fd:	00 00 00 00 00 
   26302:	48 c7 84 24 88 00 00 	movq   $0x0,0x88(%rsp)
   26309:	00 00 00 00 00 
   2630e:	48 c7 84 24 80 00 00 	movq   $0x0,0x80(%rsp)
   26315:	00 00 00 00 00 
   2631a:	48 c7 44 24 78 00 00 	movq   $0x0,0x78(%rsp)
   26321:	00 00 
   26323:	48 c7 44 24 70 00 00 	movq   $0x0,0x70(%rsp)
   2632a:	00 00 
   2632c:	48 c7 44 24 68 00 00 	movq   $0x0,0x68(%rsp)
   26333:	00 00 
   26335:	48 c7 44 24 60 00 00 	movq   $0x0,0x60(%rsp)
   2633c:	00 00 
   2633e:	48 89 44 24 50       	mov    %rax,0x50(%rsp)
   26343:	48 89 4c 24 58       	mov    %rcx,0x58(%rsp)
   26348:	83 7f 58 ff          	cmpl   $0xffffffff,0x58(%rdi)
   2634c:	0f 84 a2 00 00 00    	je     263f4 <<terminal_sdk::host::FrontendHost>::poll_frontend+0x164>
   26352:	48 8b 87 c0 00 00 00 	mov    0xc0(%rdi),%rax
   26359:	48 89 44 24 20       	mov    %rax,0x20(%rsp)
   2635e:	48 8b 87 b0 00 00 00 	mov    0xb0(%rdi),%rax
   26365:	48 8b 8f b8 00 00 00 	mov    0xb8(%rdi),%rcx
   2636c:	48 89 4c 24 18       	mov    %rcx,0x18(%rsp)
   26371:	48 89 44 24 10       	mov    %rax,0x10(%rsp)
   26376:	48 8b 97 80 00 00 00 	mov    0x80(%rdi),%rdx
   2637d:	48 89 fb             	mov    %rdi,%rbx
   26380:	48 8b 8f 88 00 00 00 	mov    0x88(%rdi),%rcx
   26387:	48 8d 44 24 60       	lea    0x60(%rsp),%rax
   2638c:	4c 8d 54 24 50       	lea    0x50(%rsp),%r10
   26391:	48 89 e7             	mov    %rsp,%rdi
   26394:	48 8d 74 24 10       	lea    0x10(%rsp),%rsi
   26399:	41 b8 1a 00 00 00    	mov    $0x1a,%r8d
   2639f:	45 31 c9             	xor    %r9d,%r9d
   263a2:	6a 50                	push   $0x50
   263a4:	50                   	push   %rax
   263a5:	6a 10                	push   $0x10
   263a7:	41 52                	push   %r10
   263a9:	e8 12 80 fe ff       	call   e3c0 <ostd::session::service_call>
   263ae:	48 83 c4 20          	add    $0x20,%rsp
   263b2:	0f b7 44 24 0e       	movzwl 0xe(%rsp),%eax
   263b7:	66 83 f8 ff          	cmp    $0xffff,%ax
   263bb:	74 3d                	je     263fa <<terminal_sdk::host::FrontendHost>::poll_frontend+0x16a>
   263bd:	66 85 c0             	test   %ax,%ax
   263c0:	75 3c                	jne    263fe <<terminal_sdk::host::FrontendHost>::poll_frontend+0x16e>
   263c2:	8b 54 24 08          	mov    0x8(%rsp),%edx
   263c6:	8d 42 e8             	lea    -0x18(%rdx),%eax
   263c9:	83 f8 39             	cmp    $0x39,%eax
   263cc:	0f 83 03 01 00 00    	jae    264d5 <<terminal_sdk::host::FrontendHost>::poll_frontend+0x245>
   263d2:	48 83 c2 e8          	add    $0xffffffffffffffe8,%rdx
   263d6:	48 8d 74 24 78       	lea    0x78(%rsp),%rsi
   263db:	48 8d 7c 24 10       	lea    0x10(%rsp),%rdi
   263e0:	e8 8b 1d fe ff       	call   8170 <<api::abi::cell_session::frontend::FrontendStatusV1>::decode>
   263e5:	66 83 7c 24 10 01    	cmpw   $0x1,0x10(%rsp)
   263eb:	75 1f                	jne    2640c <<terminal_sdk::host::FrontendHost>::poll_frontend+0x17c>
   263ed:	0f b7 44 24 12       	movzwl 0x12(%rsp),%eax
   263f2:	eb 0a                	jmp    263fe <<terminal_sdk::host::FrontendHost>::poll_frontend+0x16e>
   263f4:	66 b8 13 00          	mov    $0x13,%ax
   263f8:	eb 04                	jmp    263fe <<terminal_sdk::host::FrontendHost>::poll_frontend+0x16e>
   263fa:	0f b7 04 24          	movzwl (%rsp),%eax
   263fe:	c1 e0 10             	shl    $0x10,%eax
   26401:	ff c0                	inc    %eax
   26403:	48 81 c4 b0 00 00 00 	add    $0xb0,%rsp
   2640a:	5b                   	pop    %rbx
   2640b:	c3                   	ret
   2640c:	8b 4c 24 48          	mov    0x48(%rsp),%ecx
   26410:	66 b8 03 00          	mov    $0x3,%ax
   26414:	48 83 f9 04          	cmp    $0x4,%rcx
   26418:	77 e4                	ja     263fe <<terminal_sdk::host::FrontendHost>::poll_frontend+0x16e>
   2641a:	ba 18 00 00 00       	mov    $0x18,%edx
   2641f:	48 8d 35 da b8 00 00 	lea    0xb8da(%rip),%rsi        # 31d00 <fminimum_numf+0x2c80>
   26426:	48 63 3c 8e          	movslq (%rsi,%rcx,4),%rdi
   2642a:	48 01 f7             	add    %rsi,%rdi
   2642d:	ff e7                	jmp    *%rdi
   2642f:	31 c0                	xor    %eax,%eax
   26431:	e9 8a 00 00 00       	jmp    264c0 <<terminal_sdk::host::FrontendHost>::poll_frontend+0x230>
   26436:	b0 05                	mov    $0x5,%al
   26438:	eb 73                	jmp    264ad <<terminal_sdk::host::FrontendHost>::poll_frontend+0x21d>
   2643a:	48 8b 74 24 18       	mov    0x18(%rsp),%rsi
   2643f:	48 8b 7c 24 20       	mov    0x20(%rsp),%rdi
   26444:	4c 8b 44 24 28       	mov    0x28(%rsp),%r8
   26449:	4c 8b 4c 24 30       	mov    0x30(%rsp),%r9
   2644e:	48 8b 54 24 38       	mov    0x38(%rsp),%rdx
   26453:	48 8b 4c 24 40       	mov    0x40(%rsp),%rcx
   26458:	49 89 fa             	mov    %rdi,%r10
   2645b:	49 09 f2             	or     %rsi,%r10
   2645e:	41 0f 94 c2          	sete   %r10b
   26462:	4d 89 cb             	mov    %r9,%r11
   26465:	4d 09 c3             	or     %r8,%r11
   26468:	41 0f 94 c3          	sete   %r11b
   2646c:	45 08 d3             	or     %r10b,%r11b
   2646f:	49 89 ca             	mov    %rcx,%r10
   26472:	49 09 d2             	or     %rdx,%r10
   26475:	41 0f 94 c2          	sete   %r10b
   26479:	45 08 da             	or     %r11b,%r10b
   2647c:	75 80                	jne    263fe <<terminal_sdk::host::FrontendHost>::poll_frontend+0x16e>
   2647e:	48 c7 43 18 01 00 00 	movq   $0x1,0x18(%rbx)
   26485:	00 
   26486:	48 89 73 20          	mov    %rsi,0x20(%rbx)
   2648a:	48 89 7b 28          	mov    %rdi,0x28(%rbx)
   2648e:	4c 89 43 30          	mov    %r8,0x30(%rbx)
   26492:	4c 89 4b 38          	mov    %r9,0x38(%rbx)
   26496:	48 c7 43 40 01 00 00 	movq   $0x1,0x40(%rbx)
   2649d:	00 
   2649e:	48 89 53 48          	mov    %rdx,0x48(%rbx)
   264a2:	b0 02                	mov    $0x2,%al
   264a4:	ba 50 00 00 00       	mov    $0x50,%edx
   264a9:	eb 15                	jmp    264c0 <<terminal_sdk::host::FrontendHost>::poll_frontend+0x230>
   264ab:	b0 04                	mov    $0x4,%al
   264ad:	48 c7 43 18 00 00 00 	movq   $0x0,0x18(%rbx)
   264b4:	00 
   264b5:	ba 40 00 00 00       	mov    $0x40,%edx
   264ba:	eb 02                	jmp    264be <<terminal_sdk::host::FrontendHost>::poll_frontend+0x22e>
   264bc:	b0 01                	mov    $0x1,%al
   264be:	31 c9                	xor    %ecx,%ecx
   264c0:	48 89 0c 13          	mov    %rcx,(%rbx,%rdx,1)
   264c4:	0f b6 c0             	movzbl %al,%eax
   264c7:	88 83 4d 01 00 00    	mov    %al,0x14d(%rbx)
   264cd:	c1 e0 08             	shl    $0x8,%eax
   264d0:	e9 2e ff ff ff       	jmp    26403 <<terminal_sdk::host::FrontendHost>::poll_frontend+0x173>
   264d5:	48 8d 0d 7c 65 0b 00 	lea    0xb657c(%rip),%rcx        # dca58 <ostd::startup::_KEEP_START+0x380>
   264dc:	bf 18 00 00 00       	mov    $0x18,%edi
   264e1:	48 89 d6             	mov    %rdx,%rsi
   264e4:	ba 50 00 00 00       	mov    $0x50,%edx
   264e9:	e8 d2 28 fe ff       	call   8dc0 <core::slice::index::slice_index_fail>
