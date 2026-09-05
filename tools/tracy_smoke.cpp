// Standalone tool/protocol validation, not instrumentation of the Rust game.
#include <chrono>
#include <cstdint>
#include <thread>
#include <tracy/Tracy.hpp>

int main()
{
    using namespace std::chrono;
    const auto deadline = steady_clock::now() + seconds(10);
    while (!TracyIsConnected) {
        if (steady_clock::now() > deadline) return 1;
        std::this_thread::sleep_for(milliseconds(10));
    }
    for (int frame = 0; frame < 200; ++frame) {
        {
            ZoneScopedN("fps-tool-smoke-work");
            std::this_thread::sleep_for(milliseconds(5));
        }
        FrameMark;
    }
    return 0;
}
