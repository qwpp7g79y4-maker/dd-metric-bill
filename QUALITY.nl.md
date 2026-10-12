# Kwaliteitsrapport

Deze pagina laat zien hoe elke release van dd-metric-bill is gecontroleerd, wat we vonden en wat we hebben opgelost. Het beschrijft onze controles. Het is geen garantie dat de software foutloos of veilig is.

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
