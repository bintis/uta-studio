#pragma once

#include <string>

namespace uta_diagnostics {

// Redirect stdout and stderr to one append-only O_DSYNC file. Diagnostic events
// additionally call fdatasync/fsync so their last complete line survives an
// abrupt machine reset whenever the filesystem and hardware honor sync writes.
void RedirectToDurableLog(const std::string& path);

void Log(const std::string& component,
         const std::string& event,
         const std::string& details = {});

void Sync();

void SetEnvironment(const char* name, const char* value);
void UnsetEnvironment(const char* name);
std::string GetEnvironment(const char* name);

} // namespace uta_diagnostics
