#include "uta_studio/diagnostics.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <ctime>
#include <fcntl.h>
#include <iomanip>
#include <iostream>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <thread>

#if defined(_WIN32)
#include <io.h>
#include <process.h>
#else
#include <unistd.h>
#endif

namespace uta_diagnostics {
namespace {

std::mutex log_mutex;
const auto process_start = std::chrono::steady_clock::now();

int StandardOutputFd() {
#if defined(_WIN32)
    return _fileno(stdout);
#else
    return STDOUT_FILENO;
#endif
}

void SyncFd(int fd) {
    if (fd < 0) return;
#if defined(_WIN32)
    _commit(fd);
#elif defined(__linux__)
    fdatasync(fd);
#else
    fsync(fd);
#endif
}

std::string TimestampUtc() {
    const auto now = std::chrono::system_clock::now();
    const std::time_t time = std::chrono::system_clock::to_time_t(now);
    const auto millis = std::chrono::duration_cast<std::chrono::milliseconds>(
        now.time_since_epoch()) % 1000;
    std::tm utc{};
#if defined(_WIN32)
    gmtime_s(&utc, &time);
#else
    gmtime_r(&time, &utc);
#endif
    std::ostringstream stream;
    stream << std::put_time(&utc, "%Y-%m-%dT%H:%M:%S")
           << '.' << std::setfill('0') << std::setw(3) << millis.count() << 'Z';
    return stream.str();
}

std::string OneLine(std::string value) {
    for (char& ch : value) {
        if (ch == '\n' || ch == '\r') ch = ' ';
    }
    return value;
}

} // namespace

void RedirectToDurableLog(const std::string& path) {
    std::cout.flush();
    std::cerr.flush();
    std::fflush(stdout);
    std::fflush(stderr);

#if defined(_WIN32)
    const int fd = _open(path.c_str(), _O_WRONLY | _O_CREAT | _O_APPEND | _O_BINARY,
                         _S_IREAD | _S_IWRITE);
    if (fd < 0) {
        throw std::runtime_error("Failed to open diagnostic log: " + path);
    }
    if (_dup2(fd, _fileno(stdout)) != 0 || _dup2(fd, _fileno(stderr)) != 0) {
        _close(fd);
        throw std::runtime_error("Failed to redirect diagnostic streams: " + path);
    }
    _close(fd);
#else
    int flags = O_WRONLY | O_CREAT | O_APPEND;
#if defined(O_DSYNC)
    flags |= O_DSYNC;
#elif defined(O_SYNC)
    flags |= O_SYNC;
#endif
    const int fd = open(path.c_str(), flags, 0644);
    if (fd < 0) {
        throw std::runtime_error("Failed to open diagnostic log: " + path);
    }
    if (dup2(fd, STDOUT_FILENO) < 0 || dup2(fd, STDERR_FILENO) < 0) {
        close(fd);
        throw std::runtime_error("Failed to redirect diagnostic streams: " + path);
    }
    close(fd);
#endif

    setvbuf(stdout, nullptr, _IOLBF, 0);
    setvbuf(stderr, nullptr, _IONBF, 0);
    std::ios::sync_with_stdio(true);
    Sync();
}

void Log(const std::string& component,
         const std::string& event,
         const std::string& details) {
    const auto elapsed = std::chrono::duration<double>(
        std::chrono::steady_clock::now() - process_start).count();
    std::ostringstream line;
    line << TimestampUtc()
         << " elapsed_s=" << std::fixed << std::setprecision(6) << elapsed;
#if defined(_WIN32)
    line << " pid=" << _getpid();
#else
    line << " pid=" << getpid();
#endif
    line << " tid=" << std::hash<std::thread::id>{}(std::this_thread::get_id())
         << " component=" << OneLine(component)
         << " event=" << OneLine(event);
    if (!details.empty()) {
        line << ' ' << OneLine(details);
    }

    std::lock_guard<std::mutex> lock(log_mutex);
    std::cout << line.str() << std::endl;
    std::cout.flush();
    SyncFd(StandardOutputFd());
}

void Sync() {
    std::cout.flush();
    std::cerr.flush();
    std::fflush(stdout);
    std::fflush(stderr);
    SyncFd(StandardOutputFd());
}

void SetEnvironment(const char* name, const char* value) {
#if defined(_WIN32)
    if (_putenv_s(name, value) != 0) {
        throw std::runtime_error(std::string("Failed to set environment variable: ") + name);
    }
#else
    if (setenv(name, value, 1) != 0) {
        throw std::runtime_error(std::string("Failed to set environment variable: ") + name);
    }
#endif
}

void UnsetEnvironment(const char* name) {
#if defined(_WIN32)
    if (_putenv_s(name, "") != 0) {
        throw std::runtime_error(std::string("Failed to clear environment variable: ") + name);
    }
#else
    if (unsetenv(name) != 0) {
        throw std::runtime_error(std::string("Failed to clear environment variable: ") + name);
    }
#endif
}

std::string GetEnvironment(const char* name) {
    const char* value = std::getenv(name);
    return value ? value : "<unset>";
}

} // namespace uta_diagnostics
