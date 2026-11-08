// Dump what the firmware's JSON parser (PinscapePico/Firmware/JSON.cpp,
// linked unmodified) makes of a file, as JSON, for differential testing
// against crates/pinscape-config (see tools/json-diff/run.py).
//   {"errors": [[line, "message"], ...], "value": <tree>}
//   tree: null | true | false | {"undef":1} | number | "string"
//         | {"arr": [tree...]} | {"obj": [["name", tree], ...]}
#include <cmath>
#include <cstdio>
#include <fstream>
#include <iterator>
#include <sstream>
#include <string>
#include "JSON.h"

static std::string Q(const char *p, size_t n)
{
    std::string r = "\"";
    for (size_t i = 0; i < n; ++i)
    {
        unsigned char c = (unsigned char)p[i];
        if (c == '"') r += "\\\"";
        else if (c == '\\') r += "\\\\";
        else if (c < 0x20 || c >= 0x7F) { char b[8]; snprintf(b, sizeof b, "\\u%04x", c); r += b; }
        else r += (char)c;
    }
    return r + "\"";
}

static void Dump(std::ostream &o, const JSONParser::Value *v)
{
    using T = JSONParser::Value::Type;
    switch (v->type)
    {
    case T::Undefined: o << "{\"undef\":1}"; break;
    case T::Null: o << "null"; break;
    case T::True: o << "true"; break;
    case T::False: o << "false"; break;
    case T::Number:
    {
        if (!std::isfinite(v->number)) { o << "\"nonfinite\""; break; }
        char b[64]; snprintf(b, sizeof b, "%.17g", v->number); o << b; break;
    }
    case T::String: o << Q(v->string.txt, v->string.len); break;
    case T::Array:
    {
        o << "{\"arr\":[";
        bool first = true;
        v->ForEach([&](int, const JSONParser::Value *e) { if (!first) o << ","; first = false; Dump(o, e); });
        o << "]}";
        break;
    }
    case T::Object:
    {
        o << "{\"obj\":[";
        bool first = true;
        v->ForEach([&](const JSONParser::Value::StringWithLen &n, const JSONParser::Value *e) {
            if (!first) o << ",";
            first = false;
            o << "[" << Q(n.txt, n.len) << ",";
            Dump(o, e);
            o << "]";
        });
        o << "]}";
        break;
    }
    }
}

int main(int argc, char **argv)
{
    if (argc != 2) return fprintf(stderr, "usage: firmware_dump <file>\n"), 2;
    std::ifstream f(argv[1], std::ios::binary);
    std::string text((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
    JSONParser p;
    p.Parse(text.c_str(), text.size());
    std::ostringstream o;
    o << "{\"errors\":[";
    bool first = true;
    for (auto &e : p.errors)
    {
        std::string m = e.message;
        while (!m.empty() && (m.back() == '\n' || m.back() == ' ')) m.pop_back();
        if (!first) o << ",";
        first = false;
        o << "[" << e.lineNum << "," << Q(m.c_str(), m.size()) << "]";
    }
    o << "],\"value\":";
    Dump(o, &p.rootValue);
    o << "}\n";
    fputs(o.str().c_str(), stdout);
    return 0;
}
