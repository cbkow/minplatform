// S9 step 4 — the OS checker called directly (NSSpellChecker): what it finds in a text and what it costs. The baseline
// that depends on no third party. Usage: swift tools/s9/spellcheck.swift <text file> [language=en]
import Cocoa
let path = CommandLine.arguments[1]
let text = try! String(contentsOfFile: path, encoding: .utf8)
let ns = text as NSString
let sc = NSSpellChecker.shared
let lang = CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "en"
print("languages available: \(sc.availableLanguages.count); automatic: \(sc.automaticallyIdentifiesLanguages); set \(lang): \(sc.setLanguage(lang))")
let tag = NSSpellChecker.uniqueSpellDocumentTag()
func run(_ name: String, _ types: NSTextCheckingResult.CheckingType) -> [NSTextCheckingResult] {
    let t = Date()
    let r = sc.check(text, range: NSRange(location: 0, length: ns.length), types: types.rawValue, options: nil, inSpellDocumentWithTag: tag, orthography: nil, wordCount: nil)
    print(String(format: "%@: %d results in %.0f ms (%d UTF-16 units)", name, r.count, Date().timeIntervalSince(t) * 1000, ns.length))
    return r
}
// Asked alone, .grammar (and .correction) return nothing: they come only together with .spelling.
let all = run("spelling + grammar + corrections", [.spelling, .grammar, .correction])
let spell = all.filter { $0.resultType == .spelling }, grammar = all.filter { $0.resultType == .grammar }
print("  of which spelling \(spell.count), grammar \(grammar.count), corrections \(all.filter { $0.resultType == .correction }.count)")
for r in all where r.resultType == .correction { print("  correction: \"\(ns.substring(with: r.range))\" → \(r.replacementString ?? "")") }
_ = run("again (warm)", [.spelling, .grammar, .correction])
var seen = Set<String>()
for r in spell { let w = ns.substring(with: r.range); if seen.insert(w).inserted { let g = sc.guesses(forWordRange: r.range, in: text, language: lang, inSpellDocumentWithTag: tag) ?? []; print("  spelling: \"\(w)\" → \(g.prefix(3))") } }
var gseen = Set<String>()
for r in grammar { for d in r.grammarDetails ?? [] { let rr = (d["NSGrammarRange"] as? NSValue)?.rangeValue ?? NSRange(); let s = ns.substring(with: NSRange(location: r.range.location + rr.location, length: min(rr.length, r.range.length - rr.location))); let key = s + "|" + ((d["NSGrammarUserDescription"] as? String) ?? ""); if gseen.insert(key).inserted, gseen.count <= 25 { print("  grammar: \"\(s)\" — \((d["NSGrammarUserDescription"] as? String) ?? "?") → \((d["NSGrammarCorrections"] as? [String]) ?? [])") } } }
// one paragraph, as an editor would re-check after an edit
let para = ns.paragraphRange(for: NSRange(location: ns.length / 2, length: 0))
let t = Date(); var n = 0
for _ in 0..<50 { n += sc.check(ns.substring(with: para), range: NSRange(location: 0, length: para.length), types: NSTextCheckingResult.CheckingType.spelling.rawValue | NSTextCheckingResult.CheckingType.grammar.rawValue, options: nil, inSpellDocumentWithTag: tag, orthography: nil, wordCount: nil).count }
print(String(format: "one paragraph (%d units), spelling + grammar: %.2f ms per check", para.length, Date().timeIntervalSince(t) * 1000 / 50))
