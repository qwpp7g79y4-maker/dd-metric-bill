# Kwaliteitsrapport

Deze pagina laat zien hoe elke release van dd-metric-bill is gecontroleerd, wat we vonden en wat we hebben opgelost. Het beschrijft onze controles. Het is geen garantie dat de software foutloos of veilig is.

## Unreleased

| | |
|---|---|
| Gecontroleerde commit | [`c0a735b`](https://github.com/qwpp7g79y4-maker/dd-metric-bill/commit/c0a735b) |
| Tests | 242 automatische tests, gedraaid in debug- en releasemodus en op Rust 1.85 (alle drie: 0 mislukt) |

### Wat is toegevoegd

- Een `fuzz/`-crate (cargo-fuzz, geen deel van het gepubliceerde package) met één target per fase die onbetrouwbare invoer verwerkt, elk met een eigen invariant in plaats van alleen de code aan te roepen:

  | Target | Invariant |
  |---|---|
  | `allow_list` | Allow-list-parsing en de Metrics without Limits-simulatie geven nooit een panic; het geïndexeerde aantal overschrijdt nooit het ingelezen aantal, per metric en in totaal |
  | `count_and_render` | `count` en het renderen naar tekst en JSON geven nooit een panic bij willekeurige samples, direct opgebouwd in plaats van via een tekstparser |
  | `diff_and_render` | `diff` en het renderen naar tekst en JSON geven nooit een panic bij willekeurige oude/nieuwe samples |
  | `parse_dogstatsd` | Het parsen van DogStatsD-tekst geeft nooit een panic bij willekeurige bytes; een geparste sample doorstaat dezelfde controles als `count_and_render` |
  | `parse_jsonl` | Het parsen van JSON-lines geeft nooit een panic bij willekeurige bytes, inclusief een `\uXXXX`-escape, een dubbele sleutel, of een waarde die meerdere regels beslaat |
  | `parse_prices` | Het parsen van prijsbestanden geeft nooit een panic bij willekeurige tekst; een kostenbedrag is altijd een simpel niet-negatief decimaal getal, nooit negatief, `NaN`, of oneindig, en `0` als het gebruik op of onder het budget blijft |
  | `parse_prometheus` | Het parsen van Prometheus-tekst geeft nooit een panic bij willekeurige bytes, inclusief onafgesloten labelwaarden, onbekende `# TYPE`-families, en namen met multi-byte UTF-8 |

  Een gedeelde controle bij de render-targets bevestigt dat geen tagwaarde lekt naar tekst- of JSON-uitvoer, en dat tekstuitvoer elk rauw stuurteken escaped behalve `\n` (JSON laat het rauwe byte staan als de JSON-specificatie escapen niet vereist, wat nog steeds geldige, parseerbare JSON is).

- Property tests (`proptest`, gedraaid onder normale `cargo test`): parsers geven nooit een panic bij willekeurige bytes; de kostenberekening komt overeen met gewone gehele-getallenrekenkunde; `count` en `diff` renderen stabiele tekst en JSON, en die JSON parseert altijd met totalen die overeenkomen met de som van de delen.
- `tests/more_edge_cases.rs`: een losse CR, een tagwaarde van 2.000.000 bytes, een NUL-byte in een metricnaam, Unicode-witruimte en op elkaar lijkende letters, en getallen zoals `1e309` en `-0` in JSONL-waarden en in prijzen.
- CI: een nachtelijke `fuzz-build`-job bouwt de 7 fuzz-targets (alleen bouwen, niet draaien); de fuzz-crate is verder uitgesloten van de stabiele CI en van het gepubliceerde package.

### Hoe we controleerden

- `cargo test`, `cargo test --release`, en `cargo +1.85 test`: 242 geslaagd, 0 mislukt, bij elke run, exit 0.
- `cargo clippy --all-targets -- -D warnings`: exit 0. `cargo fmt --check`, in de hoofdcrate en in `fuzz/`: exit 0.
- `cargo +nightly fuzz build`: exit 0.
- Elk van de 7 fuzz-targets draaide 660 seconden op één vastgezette CPU-kern, achter elkaar (in totaal ongeveer 77 minuten):

  | Target | Seconden | Uitvoeringen | Crashes |
  |---|---|---|---|
  | `allow_list` | 661 | 3.478.548 | 0 |
  | `count_and_render` | 662 | 798.406 | 0 |
  | `diff_and_render` | 661 | 680.315 | 0 |
  | `parse_dogstatsd` | 662 | 3.279.909 | 0 |
  | `parse_jsonl` | 661 | 29.882.236 | 0 |
  | `parse_prices` | 661 | 15.151.042 | 0 |
  | `parse_prometheus` | 662 | 4.939.565 | 0 |

- CI op deze commit: [run 38991624843](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/runs/38991624843), voltooid, succesvol.

### Echte bugs gevonden

Geen. Dit werk heeft een controle in de testharnas zelf gecorrigeerd: een C1-stuurteken mag rauw blijven staan in geldige JSON, dus de regel "elk stuurteken wordt geescaped" die de harnas afdwingt geldt alleen voor tekstuitvoer, niet voor JSON. Dat is een correctie van de testharnas, geen productbug.

### Open punten

- De fuzz-runs hierboven zijn korte lokale runs (11 minuten per target, elk op één kern), geen continue fuzzing, en startten elke keer met een lege corpus.
- CI bouwt de fuzz-targets alleen; het draait ze niet, dus CI alleen zou een crash die een lokale fuzz-run wel vindt niet opvangen.
- `tests/more_edge_cases.rs` documenteert een inconsistentie in plaats van die op te lossen: een prijs geschreven als het kale gehele getal `-0` wordt geaccepteerd als prijs `0`, terwijl `-0.0` en `-0e0` worden geweigerd als negatief. De berekende kosten zijn in beide gevallen `0`.
- Dit onderdeel is geen eindreview van deze versie; een verse eindreview volgt na deze commit, tegen de vaste edge-case-checklist die ook bij vorige versies is gebruikt. De releasebadge verandert niet door dit onderdeel.

## v0.1.2

| | |
|---|---|
| Versie | 0.1.2 ([release](https://github.com/qwpp7g79y4-maker/dd-metric-bill/releases/tag/v0.1.2), [crates.io](https://crates.io/crates/dd-metric-bill/0.1.2)) |
| Gecontroleerde commit | [`b1474ab`](https://github.com/qwpp7g79y4-maker/dd-metric-bill/commit/b1474ab) |
| Datum | 2026-10-15 |
| Tests | 229 automatische tests, gedraaid in debug- en releasemodus |

### Hoe we controleerden

- Dezelfde controles als bij de vorige versie (hieronder), op elk platform.
- Een tweede externe geautomatiseerde codereview. Elke bevinding die we konden reproduceren is in deze versie opgelost, elk met een regressietest met precies die invoer.
- Geautomatiseerde tests in CI: [run 38629049504](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/runs/38629049504) op Linux, Windows, macOS op Apple Silicon en Intel, en Rust 1.85, plus `cargo audit` (geen bekende kwetsbaarheden).
- Een verse onafhankelijke eindreview op deze commit tegen een vaste edge-case-checklist (elk punt met een concrete kwade invoer). Die eindigde op DONE; geen blokkerende bevindingen.

### Wat we vonden en oplosten

| Probleem | Oplossing |
|---|---|
| Prijzen gingen via binaire drijvende komma | Prijzen zijn exacte decimalen; meer dan 12 decimalen of buiten bereik geeft exit 2 |
| `diff` kon een panic geven bij zeer grote totalen | Het geeft een nette foutmelding |
| Een Prometheus-histogram en een counter met botsende seriesnamen werden dubbel geteld | Ze tellen één keer, en `diff`-totalen kloppen met de deltas per metric |
| Een volle schijf gaf een panic, en `--help`/`--version` gaven exit 0 als de uitvoer mislukte | Nette foutmeldingen; een mislukte schrijfactie geeft nooit exit 0 |
| Stuurtekens uit een prijsbestand of metricnamen kwamen rauw in de terminal | Ze worden ge-escaped in tekstuitvoer en op stderr |
| Een pad dat door een FIFO werd vervangen kon de tool laten wachten; veel herhaalde regels lieten het geheugen groeien; de tijd groeide kwadratisch met het aantal unieke metricnamen | Het geopende bestand wordt gecontroleerd; geheugen is begrensd; zoeken is lineair |
| Dubbele JSON-sleutels in een sample of prijsbestand werden stil opgelost (laatste wint), wat het factureerbare totaal kon veranderen | Dubbele sleutels worden geweigerd met exit 2; regressietests dekken samples, prijsbestanden, sleutelvolgorde en geneste objecten |

Alle fixes staan in de [changelog](https://github.com/qwpp7g79y4-maker/dd-metric-bill/blob/main/CHANGELOG.md).

## v0.1.1

| | |
|---|---|
| Versie | 0.1.1 ([release](https://github.com/qwpp7g79y4-maker/dd-metric-bill/releases/tag/v0.1.1), [crates.io](https://crates.io/crates/dd-metric-bill/0.1.1)) |
| Gecontroleerde commit | [`62f7016`](https://github.com/qwpp7g79y4-maker/dd-metric-bill/commit/62f7016) |
| Datum | 2026-10-11 |
| Tests | 192 automatische tests |

### Wat we controleerden

**Codereview.** De code is gelezen aan de hand van negen vaste vragen:
1. Geeft het het juiste antwoord?
2. Wat gebeurt er bij foute of ontbrekende invoer?
3. Wat gebeurt er als het halverwege misgaat, bijvoorbeeld als de uitvoer niet geschreven kan worden?
4. Welke aannames doet het, en staan die beschreven?
5. Is het veilig als dingen tegelijk gebeuren?
6. Kan onbetrouwbare invoer in de uitvoer lekken of schade doen, bijvoorbeeld via verborgen tekens?
7. Blijft het snel en zuinig bij grote invoer?
8. Is elke bewering gedekt door een test?
9. Is de code goed te lezen en aan te passen?

**Invoer van elk platform.** UTF-8 met en zonder byte order mark, UTF-16 en UTF-32, Windows-regeleinden (CRLF) en bestandspaden die geen geldige UTF-8 zijn.

**Tests op elk platform.** Elke wijziging draait alle tests op Linux, Windows, macOS op Apple Silicon en macOS op Intel, plus de oudste ondersteunde Rust-versie (1.85).

**Release.** De release wordt pas gebouwd als de tests op de getagde commit opnieuw slagen. Elke download wordt voor publicatie uitgepakt en één keer gestart, en elk archief krijgt een SHA-256-checksum.

**Afhankelijkheden.** `cargo audit` op 2026-10-12 tegen de RustSec-advisorydatabase (1296 meldingen): geen bekende kwetsbaarheden of waarschuwingen in de 29 crate-afhankelijkheden. Dit draait nu bij elke wijziging in CI.

### Hoe we controleerden

- Onafhankelijke codereview, door reviewers die de code niet schreven.
- Een externe automatische codereview. Al zijn bevindingen zijn in deze versie opgelost.
- Automatische tests in CI. Runs: [release v0.1.1](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/runs/38164324628), [CI op main](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/workflows/ci.yml?query=branch%3Amain).
- Controle van afhankelijkheden met `cargo audit`.

We noemen de reviewdiensten hier niet. Het gaat om de controles en hun uitkomst, en alles hieronder is na te gaan in de code en de changelog.

### Wat we vonden en oplosten

| Probleem | Oplossing |
|---|---|
| Op Windows noemde de gebruiksregel in foutmeldingen `dd-metric-bill.exe` | Hij noemt `dd-metric-bill` op elk platform |
| Een prijsbestand dat begint met een UTF-8 byte order mark werd geweigerd | Het wordt gelezen, net als steekproeven al werden |
| Release-archieven werden niet getest vóór publicatie | Elk archief wordt eerst uitgepakt en gestart met `--version`, nadat alle tests op de getagde commit slagen |

Alle fixes zitten in v0.1.1, zie de [changelog](https://github.com/qwpp7g79y4-maker/dd-metric-bill/blob/main/CHANGELOG.md#011---2026-10-11).

### Niet gedekt

- De telling is een schatting op basis van een steekproef die je zelf opgeeft. Het is geen officiële rekening.

## Een probleem melden

Open een [issue](https://github.com/qwpp7g79y4-maker/dd-metric-bill/issues), of volg bij beveiligingsproblemen [SECURITY.md](SECURITY.md). Contact: pep_ai@icloud.com.
