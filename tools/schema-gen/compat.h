// Portability shim so the upstream (MSVC-oriented) ConfigDocParser.cpp
// compiles unmodified with GCC/Clang. Force-included via -include.
#pragma once
#include <cstring>
#include <cctype>
#include <functional>
#include <algorithm>
#ifndef _WIN32
#include <strings.h>
#define _stricmp strcasecmp
#define _strnicmp strncasecmp
#endif
// MSVC's internal std::string::_Starts_with == C++20 starts_with
#define _Starts_with starts_with
