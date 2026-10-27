// Compile against the patched renderer's src/ directory; no display/audio required.
#include "WallpaperEngine/Application/PauseControl.h"
#include <cassert>
#include <fstream>

int main () {
    char directory[] = "/tmp/wallweave-native-pause-XXXXXX";
    assert (mkdtemp (directory));
    const auto path = std::filesystem::path (directory) / "state";
    using WallpaperEngine::Application::readPauseControl;
    assert (!readPauseControl (path));
    for (const auto value : { "", "play\n", "invalid", "pause\nextra" }) {
        std::ofstream (path) << value;
        assert (!readPauseControl (path));
    }
    std::ofstream (path) << "pause\n";
    assert (readPauseControl (path));
    const timespec old[2] { { std::time (nullptr) - 10, 0 }, { std::time (nullptr) - 10, 0 } };
    assert (utimensat (AT_FDCWD, path.c_str (), old, 0) == 0);
    assert (!readPauseControl (path));
    std::ofstream (path) << "pause\n";
    assert (readPauseControl (path));
    std::filesystem::remove (path);
    assert (mkfifo (path.c_str (), 0600) == 0);
    assert (!readPauseControl (path));
    std::filesystem::remove_all (directory);
}
