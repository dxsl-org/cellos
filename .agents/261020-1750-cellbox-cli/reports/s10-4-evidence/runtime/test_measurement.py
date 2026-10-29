import io, struct, unittest
from observer import Observer, walk_page, elf_layout
from common import Stop

class PageWalkTests(unittest.TestCase):
    def tables(self,va,large=None):
        pages={p:bytearray(4096) for p in [0x1000,0x2000,0x3000,0x4000]}
        for table,shift,nxt in [(0x1000,39,0x2000),(0x2000,30,0x3000),(0x3000,21,0x4000)]:
            if large==shift:
                struct.pack_into('<Q',pages[table],((va>>shift)&511)*8,0x40000000|0x87);return pages
            struct.pack_into('<Q',pages[table],((va>>shift)&511)*8,nxt|7)
        struct.pack_into('<Q',pages[0x4000],((va>>12)&511)*8,0x9000|7)
        return pages
    def test_present_page_offset_and_cached_tables(self):
        va=0x100002345;pages=self.tables(va);calls=[]
        def read(p,n):calls.append(p);self.assertEqual(n,4096);return bytes(pages[p])
        cache={};pa,witness=walk_page(va,0x1003,read,cache)
        self.assertEqual(pa,0x9345);self.assertEqual(len(witness),4)
        self.assertEqual(walk_page(va+1,0x1003,read,cache)[0],0x9346);self.assertEqual(len(calls),4)
    def test_missing_mapping_has_no_fabricated_physical_zero(self):
        pa,w=walk_page(0x1000,0x1000,lambda p,n:bytes(4096),{})
        self.assertIsNone(pa);self.assertEqual(len(w),1)
    def test_large_mappings_preserve_offsets(self):
        for shift in [21,30]:
            va=0x100123456;pages=self.tables(va,shift)
            pa,w=walk_page(va,0x1000,lambda p,n:bytes(pages[p]),{})
            self.assertEqual(pa,0x40000000|(va&((1<<shift)-1)))
            self.assertEqual(w[-1]['shift'],shift)

class CounterBehaviorTests(unittest.TestCase):
    def observer(self,values):
        o=object.__new__(Observer);o.phase='work';o.events=io.StringIO();o.counters=0
        class Client:
            def read_memory(self,addr,n):return next(values).to_bytes(n,'little')
        o.client=Client();return o
    def instance(self):return {'id':1,'heap':0x1000,'peak_aligned_used_bytes':0,'phase_peaks':{},'post_events':0}
    def test_peak_preserves_transient_value_after_free(self):
        o=self.observer(iter([24,80,8]));i=self.instance()
        for pc in [1,2,3]:o._counter(i,pc,'execute-post')
        self.assertEqual(i['peak_aligned_used_bytes'],80);self.assertEqual(i['last_value'],8)
        self.assertEqual(i['phase_peaks']['work'],80);self.assertEqual(i['post_events'],3)
    def test_counter_outside_arena_is_stop_not_clamp(self):
        o=self.observer(iter([6*1024*1024+1]));i=self.instance()
        with self.assertRaises(Stop):o._counter(i,1,'execute-post')
        self.assertEqual(i['post_events'],0)
    def test_finite_slot_entry_census_boundaries(self):
        o=object.__new__(Observer)
        self.assertTrue(o.is_entry(0x100000000));self.assertTrue(o.is_entry(0x100000000+511*32*1024*1024))
        self.assertFalse(o.is_entry(0x100000001));self.assertFalse(o.is_entry(0x100000000+512*32*1024*1024));self.assertFalse(o.is_entry(0))

class InterleavedEchoTests(unittest.TestCase):
    def test_exact_fragments_across_existing_service_line_retained(self):
        from common import exact_echo_output
        command='top --records -d 1 --frames 2 | to jsonl | cat';bg='[net-bridge] NIC driver reply timeout; frame not acknowledged'
        self.assertEqual(exact_echo_output(['top --reco',bg,command[10:],'json','Cellos > '],command),[bg,'json'])
    def test_arbitrary_nonservice_line_cannot_be_ignored(self):
        from common import exact_echo_output
        with self.assertRaises(Stop):exact_echo_output(['ec','arbitrary output','ho x','Cellos > '],'echo x')
    def test_truncated_echo_cannot_match_command(self):
        from common import exact_echo_output
        with self.assertRaises(Stop):exact_echo_output(['top --reco','Cellos > '],'top --records')
    def test_background_after_prompt_retained_and_exact_prompt_removed(self):
        from common import exact_echo_output
        self.assertEqual(exact_echo_output(['echo x','x','Cellos > ','[serial] ready'],'echo x'),['x','[serial] ready'])

class NativeStepObserverTests(unittest.TestCase):
    def observer(self,positions,replies):
        import threading
        o=object.__new__(Observer);o.done=threading.Event();o.breaks={0x1000,0x2000};o.handled=[];o.steps=[]
        class Client:
            def __init__(self):self.positions=iter(positions);self.replies=iter(replies)
            def read_register(self,name):return next(self.positions)
            def step_over_breakpoint(self,pc):o.steps.append(pc);return next(self.replies)
        o.client=Client();o._handle=lambda reply:o.handled.append(reply);o._pending=lambda:None
        return o
    def test_entry_step_returns_without_handling_unenrolled_next_pc(self):
        o=self.observer([0x1000,0x1004],[b'T05thread:1;']);o._stopped(b'T05thread:1;')
        self.assertEqual(o.steps,[0x1000]);self.assertEqual(len(o.handled),1)
    def test_native_write_watch_is_handled_before_further_progress(self):
        o=self.observer([0x1000,0x2000,0x2004],[b'T05watch:3000;thread:1;',b'T05thread:1;']);o._stopped(b'T05thread:1;')
        self.assertEqual(o.steps,[0x1000,0x2000]);self.assertEqual(o.handled,[b'T05thread:1;',b'T05watch:3000;thread:1;'])
    def test_unexpected_step_signal_stops_not_continues(self):
        o=self.observer([0x1000],[b'T0bthread:1;'])
        with self.assertRaisesRegex(Stop,'unexpected signal'):o._stopped(b'T05thread:1;')
        self.assertEqual(o.steps,[0x1000])

class StartupObservationTests(unittest.TestCase):
    def test_coalesced_init_log_keeps_real_prompt_event(self):
        from common import first_exact_prompt, D
        sh=D.Shell(None);raw=b'USER: === ready ===\nUSER: Cellos > \nUSER: Init: service exited cleanly\n'
        self.assertFalse(sh.idle(raw=raw,count=1))
        end=first_exact_prompt(sh,raw)
        self.assertEqual(raw[:end],b'USER: === ready ===\nUSER: Cellos > ')
    def test_arbitrary_substring_is_not_prompt(self):
        from common import first_exact_prompt, D
        self.assertIsNone(first_exact_prompt(D.Shell(None),b'USER: echoed Cellos > \nUSER: unrelated\n'))
    def test_partial_prompt_does_not_finish_timestamp(self):
        from common import first_exact_prompt, D
        self.assertIsNone(first_exact_prompt(D.Shell(None),b'USER: Cellos >'))

if __name__=='__main__':unittest.main()
