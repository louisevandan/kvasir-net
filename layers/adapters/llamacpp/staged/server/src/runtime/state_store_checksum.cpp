#include "state_store_checksum.hpp"

#include <array>
#include <iomanip>
#include <sstream>

namespace staged::runtime::detail {
namespace {

class Sha256 final {
public:
    void update(const std::uint8_t *data, std::size_t size) {
        for (std::size_t i = 0; i < size; ++i) {
            block_[length_++ & 63U] = data[i];
            if ((length_ & 63U) == 0) transform();
        }
        total_ += size;
    }

    std::array<std::uint8_t, 32> finish() {
        const auto bit_length = total_ * 8U;
        block_[length_++ & 63U] = 0x80;
        while ((length_ & 63U) != 56U) block_[length_++ & 63U] = 0;
        for (unsigned shift = 56; shift < 64; shift -= 8) {
            block_[length_++ & 63U] = static_cast<std::uint8_t>(bit_length >> shift);
        }
        transform();
        std::array<std::uint8_t, 32> result{};
        for (unsigned i = 0; i < 8; ++i) {
            result[i * 4] = static_cast<std::uint8_t>(state_[i] >> 24U);
            result[i * 4 + 1] = static_cast<std::uint8_t>(state_[i] >> 16U);
            result[i * 4 + 2] = static_cast<std::uint8_t>(state_[i] >> 8U);
            result[i * 4 + 3] = static_cast<std::uint8_t>(state_[i]);
        }
        return result;
    }

private:
    static constexpr std::array<std::uint32_t, 64> k = {
        0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,
        0x923f82a4,0xab1c5ed0,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,
        0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,0xe49b69c1,0xefbe4786,
        0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
        0x983e5152,0xa831c66b,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,
        0x06ca6351,0x14292967,0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,
        0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,
        0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3580,0x106aa070,
        0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,
        0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,
        0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2};
    std::array<std::uint32_t, 8> state_ = {
        0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,
        0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19};
    std::array<std::uint8_t, 64> block_{};
    std::size_t length_ = 0;
    std::uint64_t total_ = 0;

    static std::uint32_t rotr(std::uint32_t x, unsigned n) {
        return (x >> n) | (x << (32U - n));
    }

    void transform() {
        std::array<std::uint32_t, 64> w{};
        for (unsigned i = 0; i < 16; ++i) {
            w[i] = (static_cast<std::uint32_t>(block_[i*4]) << 24U)
                | (static_cast<std::uint32_t>(block_[i*4+1]) << 16U)
                | (static_cast<std::uint32_t>(block_[i*4+2]) << 8U)
                | block_[i*4+3];
        }
        for (unsigned i = 16; i < 64; ++i) {
            const auto s0 = rotr(w[i-15], 7) ^ rotr(w[i-15], 18) ^ (w[i-15] >> 3U);
            const auto s1 = rotr(w[i-2], 17) ^ rotr(w[i-2], 19) ^ (w[i-2] >> 10U);
            w[i] = w[i-16] + s0 + w[i-7] + s1;
        }
        auto a = state_[0], b = state_[1], c = state_[2], d = state_[3];
        auto e = state_[4], f = state_[5], g = state_[6], h = state_[7];
        for (unsigned i = 0; i < 64; ++i) {
            const auto s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
            const auto ch = (e & f) ^ (~e & g);
            const auto t1 = h + s1 + ch + k[i] + w[i];
            const auto s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
            const auto maj = (a & b) ^ (a & c) ^ (b & c);
            const auto t2 = s0 + maj;
            h=g; g=f; f=e; e=d+t1; d=c; c=b; b=a; a=t1+t2;
        }
        state_[0]+=a; state_[1]+=b; state_[2]+=c; state_[3]+=d;
        state_[4]+=e; state_[5]+=f; state_[6]+=g; state_[7]+=h;
    }
};

} // namespace

std::string checksum_hex(const std::vector<std::uint8_t> &bytes) {
    Sha256 sha;
    if (!bytes.empty()) sha.update(bytes.data(), bytes.size());
    const auto digest = sha.finish();
    std::ostringstream out;
    out << std::hex << std::setfill('0');
    for (const auto value : digest) out << std::setw(2) << static_cast<unsigned>(value);
    return out.str();
}

} // namespace staged::runtime::detail
