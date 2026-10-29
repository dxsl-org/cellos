
/tmp/cellbox-s10-3d-resume/cand/desktop.kernel.elf:     file format elf64-x86-64


Disassembly of section .text:

ffffffff80056e90 <<cellos_kernel::task::cell_session::State>::endpoint_index>:
ffffffff80056e90:	48 85 d2             	test   %rdx,%rdx
ffffffff80056e93:	74 61                	je     ffffffff80056ef6 <<cellos_kernel::task::cell_session::State>::endpoint_index+0x66>
ffffffff80056e95:	48 83 fa 01          	cmp    $0x1,%rdx
ffffffff80056e99:	75 10                	jne    ffffffff80056eab <<cellos_kernel::task::cell_session::State>::endpoint_index+0x1b>
ffffffff80056e9b:	48 39 4e 38          	cmp    %rcx,0x38(%rsi)
ffffffff80056e9f:	75 55                	jne    ffffffff80056ef6 <<cellos_kernel::task::cell_session::State>::endpoint_index+0x66>
ffffffff80056ea1:	31 c0                	xor    %eax,%eax
ffffffff80056ea3:	4c 39 46 40          	cmp    %r8,0x40(%rsi)
ffffffff80056ea7:	74 43                	je     ffffffff80056eec <<cellos_kernel::task::cell_session::State>::endpoint_index+0x5c>
ffffffff80056ea9:	eb 4b                	jmp    ffffffff80056ef6 <<cellos_kernel::task::cell_session::State>::endpoint_index+0x66>
ffffffff80056eab:	31 c0                	xor    %eax,%eax
ffffffff80056ead:	0f 1f 00             	nopl   (%rax)
ffffffff80056eb0:	49 89 c1             	mov    %rax,%r9
ffffffff80056eb3:	49 89 d2             	mov    %rdx,%r10
ffffffff80056eb6:	49 d1 ea             	shr    $1,%r10
ffffffff80056eb9:	4c 01 d0             	add    %r10,%rax
ffffffff80056ebc:	4c 69 d8 98 00 00 00 	imul   $0x98,%rax,%r11
ffffffff80056ec3:	4a 39 4c 1e 38       	cmp    %rcx,0x38(%rsi,%r11,1)
ffffffff80056ec8:	49 0f 47 c1          	cmova  %r9,%rax
ffffffff80056ecc:	4c 29 d2             	sub    %r10,%rdx
ffffffff80056ecf:	48 83 fa 01          	cmp    $0x1,%rdx
ffffffff80056ed3:	77 db                	ja     ffffffff80056eb0 <<cellos_kernel::task::cell_session::State>::endpoint_index+0x20>
ffffffff80056ed5:	48 69 d0 98 00 00 00 	imul   $0x98,%rax,%rdx
ffffffff80056edc:	48 39 4c 16 38       	cmp    %rcx,0x38(%rsi,%rdx,1)
ffffffff80056ee1:	75 13                	jne    ffffffff80056ef6 <<cellos_kernel::task::cell_session::State>::endpoint_index+0x66>
ffffffff80056ee3:	48 01 d6             	add    %rdx,%rsi
ffffffff80056ee6:	4c 39 46 40          	cmp    %r8,0x40(%rsi)
ffffffff80056eea:	75 0a                	jne    ffffffff80056ef6 <<cellos_kernel::task::cell_session::State>::endpoint_index+0x66>
ffffffff80056eec:	48 89 47 08          	mov    %rax,0x8(%rdi)
ffffffff80056ef0:	31 c0                	xor    %eax,%eax
ffffffff80056ef2:	66 89 07             	mov    %ax,(%rdi)
ffffffff80056ef5:	c3                   	ret
ffffffff80056ef6:	66 c7 47 02 07 00    	movw   $0x7,0x2(%rdi)
ffffffff80056efc:	66 b8 01 00          	mov    $0x1,%ax
ffffffff80056f00:	66 89 07             	mov    %ax,(%rdi)
ffffffff80056f03:	c3                   	ret
