// What may be emitted at a token boundary, and what may not.
//
// A token boundary is not a character boundary, so the tail of a detokenised
// piece can be the first bytes of a character whose rest is in the next token.
// Three defects came out of getting this wrong, and these fix the contract:
//
//   1. The physical path skipped the trim at a terminal, meaning to flush what
//      was pending. A 35B run that stopped on its length budget mid-character
//      then failed validation, and the fallback replaced a whole 1,396-character
//      answer with one replacement character.
//   2. The fallback itself was wrong. U+FFFD is what the acceptance judge reads
//      as "a token was split across a stage boundary", so emitting it turned
//      local truncation into a false report about the pipeline - and hid the
//      damage while doing it.
//   3. The two hop paths fixed the streaming branch and left the terminal one
//      accumulating text with no trim and no check at all.
//
// The helpers below are what all three call, so this pins them directly.

#include "llama_stage_runtime_hop_shared.hpp"

#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <iostream>
#include <string>

namespace {

using staged::llama_runtime::hop::complete_utf8_prefix;
using staged::llama_runtime::hop::valid_utf8_text;

// Two three-byte Korean syllables, as bytes so the host code page cannot
// change what this file means: U+D55C and U+AC00.
const std::string HAN = "\xED\x95\x9C";
const std::string GA = "\xEA\xB0\x80";

void whole_text_passes_through() {
    for (const auto & text : {std::string(), std::string("plain ascii"), HAN + GA,
                              std::string("mixed ") + HAN + " tail"}) {
        assert(valid_utf8_text(text));
        assert(complete_utf8_prefix(text) == text.size());
    }
}

void an_incomplete_tail_is_held_back() {
    // One byte of a three-byte character, then two: both are the beginning of
    // something whose remainder is in the next token.
    for (std::size_t held = 1; held < HAN.size(); ++held) {
        const auto text = GA + HAN.substr(0, held);
        assert(complete_utf8_prefix(text) == GA.size());
        // What survives the trim is whole, which is the property the callers
        // rely on when they refuse whatever is invalid afterwards.
        assert(valid_utf8_text(text.substr(0, complete_utf8_prefix(text))));
    }
}

void a_trimmed_prefix_is_never_a_replacement_character() {
    // The regression: generation stops mid-character on a length budget, so
    // there is no next token to complete it. Trimming leaves the valid part
    // whole - the old code left the invalid tail on, failed validation, and
    // replaced the entire answer with U+FFFD.
    // The Korean is written as bytes on purpose: MSVC reads this file in the
    // host code page, and a literal in it becomes a compile error on a Korean
    // Windows rather than the string the test means.
    const std::string prefix = "\xEC\x9E\x91\xEC\x9D\x80\x20\xED\x94\x84\xEB\xA1\x9C\xEC\xA0\x9D\xED\x8A\xB8\xEC\x97\x90\xEC\x84\x9C\xEB\x8A\x94";
    const std::string answer = prefix + HAN.substr(0, 2);
    const auto kept = answer.substr(0, complete_utf8_prefix(answer));
    assert(valid_utf8_text(kept));
    assert(kept == prefix);
    assert(kept.find("\xEF\xBF\xBD") == std::string::npos);
}

void corruption_is_not_an_incomplete_tail() {
    // A continuation byte with no lead, and a lead byte followed by ASCII.
    // Neither is the beginning of anything, so trimming cannot rescue them and
    // the callers must refuse rather than cut until something validates.
    for (const auto & text : {std::string("\x80") + "abc",
                              std::string("\xED") + "abc",
                              std::string("\xED\x95") + "abc"}) {
        assert(!valid_utf8_text(text));
        const auto kept = text.substr(0, complete_utf8_prefix(text));
        assert(!valid_utf8_text(kept) || kept.size() < text.size());
    }
}

void bytes_that_cannot_lead_are_refused() {
    // Each of these looked like a lead to the old validator because it took
    // the width from the byte before asking whether the byte could begin a
    // character at all. A detokeniser that produced any of them is corrupt,
    // not unfinished, and the callers refuse on this answer.
    assert(!valid_utf8_text("\x80\x80"));            // continuation, nothing to continue
    assert(!valid_utf8_text("\xBF\x80"));            // the last continuation byte
    assert(!valid_utf8_text("\xC0\x80"));            // overlong NUL
    assert(!valid_utf8_text("\xC1\xBF"));            // overlong 0x7F
    assert(!valid_utf8_text("\xF5\x80\x80\x80"));    // past U+10FFFF
    assert(!valid_utf8_text("\xFF"));                // never legal anywhere
}

void surrogates_and_overlongs_are_refused() {
    // Legal leads whose second byte makes the character illegal: a UTF-16
    // surrogate, an overlong three-byte form, an overlong four-byte form, and
    // a code point above U+10FFFF.
    assert(!valid_utf8_text("\xED\xA0\x80"));
    assert(!valid_utf8_text("\xE0\x80\x80"));
    assert(!valid_utf8_text("\xF0\x80\x80\x80"));
    assert(!valid_utf8_text("\xF4\x90\x80\x80"));
    // And the widest legal character still passes.
    assert(valid_utf8_text("\xF4\x8F\xBF\xBF"));
}

}  // namespace

int main() {
    whole_text_passes_through();
    an_incomplete_tail_is_held_back();
    a_trimmed_prefix_is_never_a_replacement_character();
    corruption_is_not_an_incomplete_tail();
    bytes_that_cannot_lead_are_refused();
    surrogates_and_overlongs_are_refused();
    std::cout << "utf8 boundary contract holds\n";
    return 0;
}
