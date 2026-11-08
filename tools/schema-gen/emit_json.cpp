// schema-gen: turn upstream Pinscape Pico's JSONConfigRef.txt into
//   * JSONConfigRef.htm  (the HTML reference, exactly as upstream generates it)
//   * schema.json        (machine-readable schema for the editor's validation,
//                         autocomplete and hover help)
//
// It links the *unmodified* upstream ConfigDocParser.cpp (its main() is renamed
// away with -Dmain=upstream_main) so the parsing rules match upstream exactly.
// Build/run via scripts/sync-upstream-assets.sh.
//
// schema.json layout:
// {
//   "source": "...", "root": 0,
//   "objects": [ { "classType": "...", "subclassSelector": "type",
//                  "subclasses": { "<selector value>": <objIndex> },
//                  "props": { "<name>": Prop } } ],
// }
// Prop: { "types": [Type], "required": bool, "link": "anchor", "title": "...",
//         "summary": "...", "doc": "...", "enum": ["..."], "validate": "regex",
//         "see": "...", "obj": <objIndex>|null }
// Type: { "kind": "object|string|number|boolean|gpio|adcgpio|array",
//         "obj": <objIndex>|null, "items": [Type] }

#include <cstdio>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>
#include "ConfigDocParser.h"   // from upstream; found via -I <PinscapePico>/ConfigDocParser

using PT = ConfigDocParser::Prop::PrimitiveType;

static const char *KindName(PT t)
{
    switch (t)
    {
    case PT::Object: return "object";
    case PT::String: return "string";
    case PT::Number: return "number";
    case PT::Bool: return "boolean";
    case PT::GPIO: return "gpio";
    case PT::ADCGPIO: return "adcgpio";
    case PT::Array: return "array";
    default: return "none";
    }
}

static std::string Q(const std::string &s)
{
    std::string r = "\"";
    for (unsigned char c : s)
    {
        switch (c)
        {
        case '"': r += "\\\""; break;
        case '\\': r += "\\\\"; break;
        case '\n': r += "\\n"; break;
        case '\r': r += "\\r"; break;
        case '\t': r += "\\t"; break;
        default:
            if (c < 0x20) { char b[8]; snprintf(b, sizeof b, "\\u%04x", c); r += b; }
            else r += (char)c;
        }
    }
    return r + "\"";
}

// Strip HTML tags and decode the few entities the reference uses.
static std::string PlainText(const std::string &html)
{
    std::string out;
    for (size_t i = 0; i < html.size();)
    {
        if (html[i] == '<')
        {
            char qu = 0;
            for (++i; i < html.size(); ++i)
            {
                if (qu) { if (html[i] == qu) qu = 0; }
                else if (html[i] == '"' || html[i] == '\'') qu = html[i];
                else if (html[i] == '>') { ++i; break; }
            }
            continue;
        }
        if (html[i] == '&')
        {
            static const std::pair<const char *, const char *> ents[] = {
                {"&lt;", "<"}, {"&gt;", ">"}, {"&amp;", "&"}, {"&quot;", "\""}, {"&nbsp;", " "}, {"&apos;", "'"}};
            bool hit = false;
            for (auto &e : ents)
            {
                size_t n = strlen(e.first);
                if (html.compare(i, n, e.first) == 0) { out += e.second; i += n; hit = true; break; }
            }
            if (hit) continue;
        }
        out += html[i++];
    }
    return out;
}

// Paragraphs of the documentation text (blank-line separated), as plain text.
static std::vector<std::string> Paragraphs(const std::list<std::string> &lines)
{
    std::vector<std::string> paras;
    std::string cur;
    for (auto &l : lines)
    {
        if (l.empty()) { if (!cur.empty()) paras.push_back(PlainText(cur)); cur.clear(); continue; }
        if (!cur.empty()) cur += ' ';
        cur += l;
    }
    if (!cur.empty()) paras.push_back(PlainText(cur));
    return paras;
}

static std::vector<const ConfigDocParser::Object *> objs;

static int ObjIndex(const ConfigDocParser::Object *o)
{
    return (o && o->objNum >= 0) ? o->objNum : -1;
}

static void Collect(const ConfigDocParser::Object &o)
{
    if (o.objNum < 0) return;
    if ((size_t)o.objNum >= objs.size()) objs.resize(o.objNum + 1, nullptr);
    if (objs[o.objNum]) return;
    objs[o.objNum] = &o;
    for (auto &p : o.props)
    {
        Collect(p.second.subObj);
        for (auto &t : p.second.types)
            if (t.xrefObj) Collect(*t.xrefObj);
    }
    for (auto &sc : o.subclassTypes) Collect(sc.second);
}

static void EmitType(std::ostream &f, const ConfigDocParser::Prop::Type &t)
{
    f << "{\"kind\":" << Q(KindName(t.primitiveType));
    int o = ObjIndex(t.xrefObj);
    if (o >= 0) f << ",\"obj\":" << o;
    if (t.primitiveType == PT::Array)
    {
        f << ",\"items\":[";
        bool first = true;
        for (auto &st : t.subTypes) { if (!first) f << ","; first = false; EmitType(f, st); }
        f << "]";
    }
    f << "}";
}

static void EmitObject(std::ostream &f, const ConfigDocParser::Object &o)
{
    f << "{";
    if (!o.classType.empty()) f << "\"classType\":" << Q(o.classType) << ",";
    if (!o.subclassSelectorPropName.empty())
    {
        f << "\"subclassSelector\":" << Q(o.subclassSelectorPropName) << ",\"subclasses\":{";
        bool first = true;
        for (auto &sc : o.subclassTypes)
        {
            if (!first) f << ",";
            first = false;
            f << Q(sc.first) << ":" << ObjIndex(&sc.second);
        }
        f << "},";
    }
    f << "\"props\":{";
    bool firstProp = true;
    for (auto &pair : o.props)
    {
        auto &p = pair.second;
        if (!firstProp) f << ",";
        firstProp = false;
        f << "\n  " << Q(pair.first) << ":{\"types\":[";
        bool first = true;
        for (auto &t : p.types) { if (!first) f << ","; first = false; EmitType(f, t); }
        f << "],\"required\":" << (p.required ? "true" : "false");
        if (!p.link.empty()) f << ",\"link\":" << Q(p.link);
        if (!p.title.empty()) f << ",\"title\":" << Q(PlainText(p.title));
        auto paras = Paragraphs(p.text);
        if (!paras.empty())
        {
            std::string summary = paras[0];
            if (summary.size() > 160) summary = summary.substr(0, 157) + "...";
            f << ",\"summary\":" << Q(summary);
            std::string doc;
            for (size_t i = 0; i < paras.size() && doc.size() < 900; ++i)
                doc += (doc.empty() ? "" : "\n\n") + paras[i];
            if (doc.size() > 1200) doc = doc.substr(0, 1197) + "...";
            f << ",\"doc\":" << Q(doc);
        }
        if (!p.enumeratedValues.empty())
        {
            f << ",\"enum\":[";
            bool fe = true;
            for (auto &v : p.enumeratedValues)
            {
                std::string val = v;
                if (val.size() >= 2 && val.front() == '"' && val.back() == '"') val = val.substr(1, val.size() - 2);
                if (!fe) f << ",";
                fe = false;
                f << Q(val);
            }
            f << "]";
        }
        if (!p.validate.empty()) f << ",\"validate\":" << Q(p.validate);
        if (!p.see.empty()) f << ",\"see\":" << Q(p.see);
        int so = (p.subObj.props.size() != 0) ? ObjIndex(&p.subObj) : -1;
        if (so >= 0) f << ",\"obj\":" << so;
        f << "}";
    }
    f << "}}";
}

int main(int argc, char **argv)
{
    if (argc != 5)
        return fprintf(stderr, "usage: schema-gen <JSONConfigRef.txt> <out.htm> <out-schema.json> <scratch.h>\n"), 1;

    ConfigDocParser parser;
    parser.ParseFile(argv[1]);
    parser.GenerateHTML(argv[2]);
    // Run upstream's C++ generator into a scratch file: it resolves
    // cross-references and numbers the objects, which we reuse below.
    parser.GenerateSchemaStructs(argv[4]);

    Collect(parser.rootObject);

    std::ofstream f(argv[3]);
    if (!f) return fprintf(stderr, "can't write %s\n", argv[3]), 1;
    f << "{\"source\":\"PinscapePico/Firmware/JSONConfigRef.txt\",\"root\":" << parser.rootObject.objNum
      << ",\"objects\":[";
    for (size_t i = 0; i < objs.size(); ++i)
    {
        if (i) f << ",";
        f << "\n";
        if (objs[i]) EmitObject(f, *objs[i]);
        else f << "{\"props\":{}}";
    }
    f << "\n]}\n";
    printf("schema-gen: %zu objects -> %s\n", objs.size(), argv[3]);
    return 0;
}
