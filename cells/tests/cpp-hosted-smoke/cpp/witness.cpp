// SPDX-License-Identifier: MPL-2.0
// `cpp-hosted` witness logic (ADR-0034, runtime plan R4). Plain hosted C++17
// over libc++/libc++abi/libunwind and mlibc `posix`; the same file runs on the
// host build (scripts/build-llvm-runtimes.sh host) and inside the Cell.
//
// Contract with the caller (Rust Cell host or host_main.c):
//   - both entry points are `extern "C"` and `noexcept`: no exception ever
//     reaches a Rust frame (ADR-0034 decision 4);
//   - integer-only signatures (rule H1): no float crosses the boundary;
//   - markers: `CPP-HOSTED: <check> OK|FAIL`, then `CPP-HOSTED: <pass>/<total> pass`.
//
// Thread budget: R3b is not in yet, so every std::thread runs on the 64 KiB
// Spawn stack and every allocation comes from mlibc's 16 MiB arena. No check
// runs more than four threads at once.

#include <algorithm>
#include <atomic>
#include <charconv>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <exception>
#include <future>
#include <iostream>
#include <map>
#include <mutex>
#include <numeric>
#include <optional>
#include <shared_mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <typeinfo>
#include <unordered_map>
#include <variant>
#include <vector>

namespace {

int g_pass = 0;
int g_total = 0;

void report(const char *name, bool ok) {
    ++g_total;
    if (ok)
        ++g_pass;
    std::printf("CPP-HOSTED: %s %s\n", name, ok ? "OK" : "FAIL");
    std::fflush(stdout);
}

bool check_containers() {
    std::vector<int> v(64);
    std::iota(v.begin(), v.end(), 1);
    std::reverse(v.begin(), v.end());
    std::sort(v.begin(), v.end());
    std::map<std::string, int> m{{"two", 2}, {"one", 1}, {"three", 3}};
    std::unordered_map<int, std::string> u;
    for (int i = 0; i < 100; ++i)
        u.emplace(i, std::to_string(i * i));
    std::string joined;
    for (const auto &[key, value] : m)
        joined += key + "=" + std::to_string(value) + ";";
    return std::accumulate(v.begin(), v.end(), 0) == 64 * 65 / 2 && v.front() == 1 &&
           joined == "one=1;three=3;two=2;" && u.at(12) == "144" && u.size() == 100;
}

bool check_optional_variant() {
    std::optional<std::string> none;
    std::optional<std::string> some = std::string("cellos");
    std::variant<int, std::string, double> var = std::string("hosted");
    bool visited = std::visit(
        [](const auto &value) {
            using T = std::decay_t<decltype(value)>;
            return std::is_same_v<T, std::string>;
        },
        var);
    var = 7;
    bool bad_access = false;
    try {
        (void)std::get<std::string>(var);
    } catch (const std::bad_variant_access &) {
        bad_access = true;
    }
    return !none && some.value_or("") == "cellos" && visited && std::get<int>(var) == 7 &&
           bad_access;
}

bool check_charconv() {
    char buf[64];
    auto [end, ec] = std::to_chars(buf, buf + sizeof buf, -123456789);
    bool int_ok = ec == std::errc() && std::string(buf, end) == "-123456789";
    unsigned parsed = 0;
    const char *hex = "ff7f";
    auto from = std::from_chars(hex, hex + 4, parsed, 16);
    bool parse_ok = from.ec == std::errc() && parsed == 0xff7f;
    // Shortest round-trip float formatting (Ryu inside libc++); SSE2 code that
    // never crosses into Rust (rule H1).
    auto dbl = std::to_chars(buf, buf + sizeof buf, 0.1);
    bool dbl_ok = dbl.ec == std::errc() && std::string(buf, dbl.ptr) == "0.1";
    return int_ok && parse_ok && dbl_ok;
}

struct WitnessError : std::runtime_error {
    int depth;
    explicit WitnessError(int d) : std::runtime_error("witness depth"), depth(d) {}
};

// Three frames between the throw and the handler, each with a destructor that
// must run during phase-2 unwinding.
int g_unwound = 0;
struct Guard {
    ~Guard() { ++g_unwound; }
};
[[gnu::noinline]] void frame3(int d) {
    Guard g;
    throw WitnessError(d);
}
[[gnu::noinline]] void frame2(int d) {
    Guard g;
    frame3(d + 1);
}
[[gnu::noinline]] void frame1(int d) {
    Guard g;
    frame2(d + 1);
}

bool check_exceptions() {
    g_unwound = 0;
    int depth = -1;
    try {
        frame1(1);
    } catch (const std::exception &e) {
        auto *we = dynamic_cast<const WitnessError *>(&e);
        depth = we ? we->depth : -2;
    }
    bool rethrown = false;
    try {
        try {
            throw std::out_of_range("inner");
        } catch (...) {
            throw;
        }
    } catch (const std::logic_error &e) {
        rethrown = std::strcmp(e.what(), "inner") == 0;
    }
    return depth == 3 && g_unwound == 3 && rethrown;
}

struct Shape {
    virtual ~Shape() = default;
    virtual int sides() const = 0;
};
struct Named {
    virtual ~Named() = default;
};
struct Square : Shape, Named {
    int sides() const override { return 4; }
};
struct Circle : Shape {
    int sides() const override { return 0; }
};

bool check_dynamic_cast() {
    Square square;
    Circle circle;
    Shape *a = &square;
    Shape *b = &circle;
    bool down = dynamic_cast<Square *>(a) != nullptr && dynamic_cast<Square *>(b) == nullptr;
    bool cross = dynamic_cast<Named *>(a) != nullptr;
    bool threw = false;
    try {
        (void)dynamic_cast<Square &>(*b);
    } catch (const std::bad_cast &) {
        threw = true;
    }
    return down && cross && threw && typeid(*a) == typeid(Square) && a->sides() == 4;
}

bool check_thread_join() {
    std::atomic<int> sum{0};
    std::vector<std::thread> threads;
    for (int t = 1; t <= 4; ++t)
        threads.emplace_back([&sum, t] {
            for (int i = 0; i < 1000; ++i)
                sum.fetch_add(t, std::memory_order_relaxed);
        });
    for (auto &th : threads)
        th.join();
    return sum.load() == 1000 * (1 + 2 + 3 + 4) &&
           std::all_of(threads.begin(), threads.end(), [](auto &th) { return !th.joinable(); });
}

bool check_future() {
    auto value = std::async(std::launch::async, [] { return 6 * 7; });
    std::promise<int> promise;
    std::future<int> failing = promise.get_future();
    std::thread setter([&promise] {
        try {
            throw std::invalid_argument("from thread");
        } catch (...) {
            promise.set_exception(std::current_exception());
        }
    });
    bool propagated = false;
    try {
        (void)failing.get();
    } catch (const std::invalid_argument &e) {
        propagated = std::strcmp(e.what(), "from thread") == 0;
    }
    setter.join();
    return value.get() == 42 && propagated;
}

bool check_shared_mutex() {
    std::shared_mutex mutex;
    std::map<int, int> table;
    std::atomic<int> reads{0};
    std::thread writer([&] {
        for (int i = 0; i < 200; ++i) {
            std::unique_lock lock(mutex);
            table[i] = i * 2;
        }
    });
    std::vector<std::thread> readers;
    for (int r = 0; r < 2; ++r)
        readers.emplace_back([&] {
            for (int i = 0; i < 200; ++i) {
                std::shared_lock lock(mutex);
                auto it = table.find(i);
                if (it == table.end() || it->second == i * 2)
                    reads.fetch_add(1, std::memory_order_relaxed);
            }
        });
    writer.join();
    for (auto &th : readers)
        th.join();
    std::shared_lock lock(mutex);
    return reads.load() == 400 && table.size() == 200 && table[199] == 398;
}

thread_local int t_counter = 100;

bool check_thread_local() {
    t_counter = 1;
    std::atomic<int> fresh{0};
    std::vector<std::thread> threads;
    for (int t = 0; t < 3; ++t)
        threads.emplace_back([&fresh, t] {
            if (t_counter == 100)
                fresh.fetch_add(1);
            t_counter = t;
        });
    for (auto &th : threads)
        th.join();
    return fresh.load() == 3 && t_counter == 1;
}

std::atomic<int> g_constructed{0};
struct Slow {
    int value;
    Slow() : value(0) {
        g_constructed.fetch_add(1);
        // Hold the guard long enough that the other threads block on it.
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
        value = 77;
    }
};

int slow_static() {
    static Slow instance;
    return instance.value;
}

bool check_static_guard() {
    std::atomic<int> seen{0};
    std::atomic<bool> go{false};
    std::vector<std::thread> threads;
    for (int t = 0; t < 4; ++t)
        threads.emplace_back([&] {
            while (!go.load())
                std::this_thread::yield();
            if (slow_static() == 77)
                seen.fetch_add(1);
        });
    go.store(true);
    for (auto &th : threads)
        th.join();
    return g_constructed.load() == 1 && seen.load() == 4;
}

bool check_cout() {
    std::ostringstream text;
    text << "fmt " << 42 << ' ' << std::hex << 255 << std::dec << ' ' << 2.5;
    std::cout << "CPP-HOSTED: cout says " << text.str() << std::endl;
    return std::cout.good() && text.str() == "fmt 42 ff 2.5";
}

} // namespace

extern "C" int cpp_hosted_witness() noexcept {
    g_pass = 0;
    g_total = 0;
    try {
        report("containers", check_containers());
        report("optional-variant", check_optional_variant());
        report("charconv", check_charconv());
        report("exceptions", check_exceptions());
        report("dynamic-cast", check_dynamic_cast());
        report("thread-join", check_thread_join());
        report("future", check_future());
        report("shared-mutex", check_shared_mutex());
        report("thread-local", check_thread_local());
        report("static-guard", check_static_guard());
        report("cout", check_cout());
    } catch (const std::exception &e) {
        std::printf("CPP-HOSTED: unexpected exception FAIL (%s)\n", e.what());
        ++g_total;
    }
    std::printf("CPP-HOSTED: %d/%d pass\n", g_pass, g_total);
    std::fflush(stdout);
    return g_total - g_pass;
}

[[gnu::noinline]] static void throw_uncaught() {
    throw std::runtime_error("cpp-hosted uncaught");
}

// The exception reaches this noexcept boundary with no handler: std::terminate,
// libc++abi names the exception on stderr, then abort() ends the Cell. Never
// returns.
extern "C" void cpp_hosted_uncaught() noexcept {
    std::printf("CPP-HOSTED: throwing uncaught\n");
    std::fflush(stdout);
    throw_uncaught();
}
