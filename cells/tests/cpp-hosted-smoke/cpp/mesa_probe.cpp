// SPDX-License-Identifier: MPL-2.0
// Size probe for a Mesa-shaped `cpp-hosted` link (ADR-0034 size table; built
// only by `scripts/build-llvm-runtimes.sh size`, never part of the Cell).
// Mesa's compiler back ends compile -fno-exceptions -fno-rtti and use these
// headers (iGPU port gaps §1.3); no iostreams, no threads.

#include <charconv>
#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <variant>
#include <vector>

namespace {

struct Instr {
    uint32_t opcode;
    std::vector<uint32_t> srcs;
    std::optional<uint32_t> dest;
};

using Operand = std::variant<uint32_t, float, std::string>;

} // namespace

extern "C" int cpp_hosted_mesa_probe(int seed) noexcept {
    std::vector<Instr> block;
    for (int i = 0; i < seed; ++i)
        block.push_back({static_cast<uint32_t>(i), {1u, 2u}, i % 2 ? std::optional<uint32_t>(i) : std::nullopt});
    std::map<std::string, int> names{{"mov", 1}, {"add", 2}};
    std::unordered_map<uint32_t, Operand> regs;
    regs.emplace(1u, Operand(std::string("r1")));
    regs.emplace(2u, Operand(1.5f));
    char text[32];
    auto printed = std::to_chars(text, text + sizeof text, seed * 3);
    int parsed = 0;
    std::from_chars(text, printed.ptr, parsed);
    std::string_view view(text, static_cast<size_t>(printed.ptr - text));
    return static_cast<int>(block.size()) + names.at("add") + static_cast<int>(regs.size()) + parsed +
           static_cast<int>(view.size()) + static_cast<int>(std::holds_alternative<float>(regs.at(2)));
}
