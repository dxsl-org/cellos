
/tmp/cellbox-s10-3d-resume/cand/desktop.kernel.elf:     file format elf64-x86-64


Disassembly of section .text:

ffffffff80060280 <<cellos_kernel::task::cell_session::State>::op_index>:
ffffffff80060280:	48 85 d2             	test   %rdx,%rdx
ffffffff80060283:	74 59                	je     ffffffff800602de <<cellos_kernel::task::cell_session::State>::op_index+0x5e>
ffffffff80060285:	48 83 fa 01          	cmp    $0x1,%rdx
ffffffff80060289:	75 0f                	jne    ffffffff8006029a <<cellos_kernel::task::cell_session::State>::op_index+0x1a>
ffffffff8006028b:	48 39 0e             	cmp    %rcx,(%rsi)
ffffffff8006028e:	75 4e                	jne    ffffffff800602de <<cellos_kernel::task::cell_session::State>::op_index+0x5e>
ffffffff80060290:	31 c0                	xor    %eax,%eax
ffffffff80060292:	4c 39 46 08          	cmp    %r8,0x8(%rsi)
ffffffff80060296:	74 3c                	je     ffffffff800602d4 <<cellos_kernel::task::cell_session::State>::op_index+0x54>
ffffffff80060298:	eb 44                	jmp    ffffffff800602de <<cellos_kernel::task::cell_session::State>::op_index+0x5e>
ffffffff8006029a:	31 c0                	xor    %eax,%eax
ffffffff8006029c:	0f 1f 40 00          	nopl   0x0(%rax)
ffffffff800602a0:	49 89 c1             	mov    %rax,%r9
ffffffff800602a3:	49 89 d2             	mov    %rdx,%r10
ffffffff800602a6:	49 d1 ea             	shr    $1,%r10
ffffffff800602a9:	4c 01 d0             	add    %r10,%rax
ffffffff800602ac:	4c 6b d8 58          	imul   $0x58,%rax,%r11
ffffffff800602b0:	4a 39 0c 1e          	cmp    %rcx,(%rsi,%r11,1)
ffffffff800602b4:	49 0f 47 c1          	cmova  %r9,%rax
ffffffff800602b8:	4c 29 d2             	sub    %r10,%rdx
ffffffff800602bb:	48 83 fa 01          	cmp    $0x1,%rdx
ffffffff800602bf:	77 df                	ja     ffffffff800602a0 <<cellos_kernel::task::cell_session::State>::op_index+0x20>
ffffffff800602c1:	48 6b d0 58          	imul   $0x58,%rax,%rdx
ffffffff800602c5:	48 39 0c 16          	cmp    %rcx,(%rsi,%rdx,1)
ffffffff800602c9:	75 13                	jne    ffffffff800602de <<cellos_kernel::task::cell_session::State>::op_index+0x5e>
ffffffff800602cb:	48 01 d6             	add    %rdx,%rsi
ffffffff800602ce:	4c 39 46 08          	cmp    %r8,0x8(%rsi)
ffffffff800602d2:	75 0a                	jne    ffffffff800602de <<cellos_kernel::task::cell_session::State>::op_index+0x5e>
ffffffff800602d4:	48 89 47 08          	mov    %rax,0x8(%rdi)
ffffffff800602d8:	31 c0                	xor    %eax,%eax
ffffffff800602da:	66 89 07             	mov    %ax,(%rdi)
ffffffff800602dd:	c3                   	ret
ffffffff800602de:	66 c7 47 02 07 00    	movw   $0x7,0x2(%rdi)
ffffffff800602e4:	66 b8 01 00          	mov    $0x1,%ax
ffffffff800602e8:	66 89 07             	mov    %ax,(%rdi)
ffffffff800602eb:	c3                   	ret
