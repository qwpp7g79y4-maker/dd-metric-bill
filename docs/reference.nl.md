# dd-metric-bill referentie

[English](reference.md) · Nederlands

Deze pagina heeft de volledige details. Voor het overzicht, zie de [README](../README.nl.md).

De tekst die de binary schrijft is Engels. De voorbeelden en de foutmeldingen hieronder staan zo, precies zoals de binary ze schrijft. Het decimaalteken is een punt (bijvoorbeeld `0.10` en `1.5`). Een komma in een getal wordt niet geaccepteerd.

## Opties

`dd-metric-bill --help` of `dd-metric-bill count --help` toont de ingebouwde hulp. Opties die niet bij `diff` horen, worden geweigerd met exitcode 3, vóórdat een sample of prijsbestand wordt gelezen. Zie [Exitcodes en gebruiksfouten](#exitcodes-en-gebruiksfouten).

| Optie | Geldt voor | Wat het doet |
| --- | --- | --- |
| `--format` | count en diff | Invoerformaat: `dogstatsd` (standaard), `jsonl` of `prometheus`. |
| `--json` | count en diff | JSON-uitvoer met `schema_version` 1. Dezelfde uitkomst als de tekst. |
| `--explain` | count | Na elke metric de regeltekst en de bron-URL. |
| `--hosts` | count, alleen met `--plan` | Aantal hosts voor de allotment. Zonder deze optie zijn de hosts de verschillende waarden van de tag `host`. |
| `--plan` | count | Allotment-plan: `pro` (100 custom metrics per host) of `enterprise` (200 per host). |
| `--histogram-aggregates` | count en diff | Histogram-aggregaten per tagcombinatie. Standaard 4. |
| `--histogram-percentiles` | count en diff | Histogram-percentielen per tagcombinatie. Standaard 1. De histogramfactor is de som van de twee. |
| `--percentiles` | count en diff | Een distribution telt als 10 per combinatie in plaats van 5. |
| `--allow-tags` | count | Komma-gescheiden tagsleutels die geïndexeerd mogen worden. Spaties rond een sleutel vallen weg. Lege stukken vallen weg. |
| `--prices` | count | Pad naar een prijsbestand (JSON). |
| `--budget` | count en diff | Exitcode 1 als het totaal (bij `diff` het nieuwe totaal) boven N ligt. Het verslag wordt wel geschreven. |
| `--top` | count en diff | Alleen de N tagsleutels die de meeste series toevoegen, per metric. |
| `--unbounded-threshold` | count | Een tagsleutel met meer dan N verschillende waarden is onbegrensd. Standaard 50. Bij `diff` wordt de vlag geweigerd, ook als de waarde 50 is. |
| `--untyped-as` | count en diff, alleen met `--format prometheus` | Een Prometheus-metric zonder type telt als `gauge` of `count`. |
| `--help` | overal | Toon hulp en exitcode 0. |
| `--version` | bovenaan | Toon de versie (`dd-metric-bill 0.1.0`) en exitcode 0. |

Alleen count accepteert --explain, --plan, --hosts, --allow-tags, --prices en --unbounded-threshold. --hosts werkt alleen samen met --plan. --untyped-as werkt alleen samen met --format prometheus.

Er is geen `--show-values`-vlag. De uitvoer toont tagsleutels en aantallen, niet de tagwaarden.

## Prijsbestand

Prijzen komen alleen uit een JSON-bestand dat je met `--prices` meegeeft. De tool haalt geen prijs op en controleert de getallen niet tegen een Datadog-pagina. De repository bevat `prices/datadog-example.json` als voorbeeld van de vorm:

```json
{
  "currency": "USD",
  "source": "https://docs.datadoghq.com/account_management/billing/custom_metrics",
  "date": "2026-10-07",
  "ingested_per_100": 0.10
}
```

Verplichte velden: `currency`, `source`, `date` en `ingested_per_100`. `indexed_per_100` mag ontbreken. Elk ander veld maakt het bestand ongeldig. `currency`, `source` en `date` zijn tekst en mogen na trimmen niet leeg zijn. Een prijs is een JSON-getal. Een komma (`0,10`) is geen geldige JSON.

Een prijs wordt exact decimaal uit het JSON-getal gelezen, niet als een binaire zwevendekommawaarde. Een prijs heeft hoogstens 12 cijfers achter de komma (`MAX_SCALE`) en een teller van hoogstens 10^18 (`MAX_NUMERATOR`). Nullen aan het eind van het decimale deel vallen weg vóór die grens. Een prijs daarbuiten maakt het prijsbestand ongeldig (exit 2, melding "field is too large"). Een negatieve prijs wordt geweigerd. Het bedrag is geheeltallige rekenkunde: metrics boven de allotment, maal de prijs per 100, gedeeld door 100. Het decimaalteken is `.`, en nullen aan het eind worden niet getoond.

Zonder `--plan` toont het bestand wel valuta, bron en datum, en is er geen kostenregel. Met `--plan` wordt elk ingesteld volume tegen dezelfde allotment gezet. Ingested wordt alleen geprijsd als `--allow-tags` gezet is. Indexed wordt alleen geprijsd als `indexed_per_100` aanwezig is. Gebruik onder de allotment geeft een kost van 0.

## Hoe het rekent

Een custom metric is één combinatie van metricnaam en tagwaarden, inclusief `host`. De volgorde van de tags telt niet. Dezelfde combinatie op een volgende regel telt één keer. De ingestuurde waarde telt niet. De pagina van Datadog zegt `Reordering tag values doesn't add uniqueness` (https://docs.datadoghq.com/account_management/billing/custom_metrics, gelezen 7 oktober 2026).

De regeltekst in de uitvoer is Engels:

- D3 COUNT, RATE and GAUGE: 1 metric per unique tag combination (B1, tabs Count/Rate and Gauge).
- D4 HISTOGRAM: per unique tag combination, the number of agent aggregates. Default 5: `max`, `median`, `avg`, `count` (histogram_aggregates) plus `95percentile` (histogram_percentiles). Configurable with `--histogram-aggregates` and `--histogram-percentiles` (B1, tab Histogram).
- D5 DISTRIBUTION: 5 per unique tag combination (`count`, `sum`, `min`, `max`, `avg`); with percentiles (`p50`, `p75`, `p90`, `p95`, `p99`), 10 per combination (B1, tab Distribution).
- D10 Prometheus histogram and summary (Latest Version): _count and _sum become count (name.count and name.sum, without le or quantile), _bucket becomes count (name.bucket, with le) and quantiles become gauge (name.quantile, with quantile). 1 per unique tag combination (B2).
- D11 Prometheus without a type: a metric with TYPE untyped or without a TYPE line counts as 0 and is shown as skipped (B3). A _count, _sum or _bucket name whose base is a counter or gauge is a separate metric without a type (D11).
- D12 OpenMetrics counter: a _total sample of a counter family counts as count on name.count, 1 per label combination (B3).

Met `--explain` komt daar bij D4 en D5 `Configured factor: N.` achter. De bron-URL is B1 voor D3, D4 en D5 (https://docs.datadoghq.com/account_management/billing/custom_metrics), B2 voor D10 (https://docs.datadoghq.com/integrations/guide/prometheus-metrics/) en B3 voor D11 en D12 (https://docs.datadoghq.com/integrations/openmetrics/).

DogStatsD-types: `c` count, `g` gauge, `h` histogram, `d` distribution, `ms` histogram, `s` gauge. Een sample rate (`|@rate`) moet boven 0 en hoogstens 1 zijn. Hij verandert de telling niet.

**Allowlist.** `--allow-tags` houdt voor de indexed telling alleen de genoemde sleutels. Ingested is de telling over alle tags. Beide gebruiken dezelfde factor. De allowlist geldt voor alle metrics tegelijk. Een lege lijst houdt geen tags.

**Allotment.** Pro is 100 per host, Enterprise is 200 per host (B1, Allocation). Hosts komen uit `--hosts` of uit de verschillende `host`-waarden in het sample. Zonder allowlist is het gebruik het totaal, en dat is indexed; de tekst zegt `usage: N (total, indexed)`. Met een allowlist worden indexed en ingested elk apart tegen de allotment gezet. Het overschot is 0 als het gebruik niet boven de allotment ligt.

**Tags.** Per sleutel: hoeveel verschillende waarden hij heeft, en hoeveel series verdwijnen als die sleutel wegvalt (`added`). `--top N` houdt de N sleutels met de grootste `added`. Sleutels sorteren op `added`, dan op het aantal waarden, dan op de naam.

**Onbegrensd.** Een sleutel is onbegrensd als hij meer verschillende waarden heeft dan `--unbounded-threshold`, of als een waarde op een patroon lijkt: een UUID (`8-4-4-4-12` hex), een hex-id van minstens 16 hex-tekens met minstens één letter a-f, een numerieke id van minstens 6 cijfers, of een podnaam `name-hash-hash` waarvan de hash-delen minstens 5 alfanumerieke tekens zijn. Redenen in de uitvoer zijn `threshold`, `pattern uuid`, `pattern hex-id`, `pattern numeric-id` en `pattern pod-name`. De waarden zelf worden niet getoond. Statuscodes zoals `200` zijn korter dan 6 cijfers en zijn dus geen numerieke id.

**Diff.** Per metricnaam en soort: oude telling, nieuwe telling en delta. Metrics die alleen in het nieuwe sample zitten, staan onder `new:`. Metrics die alleen in het oude sample zitten, staan onder `removed:`. Tagsleutels tonen oude en nieuwe waarden en toegevoegde series. `--budget` vergelijkt met het nieuwe totaal.

**Overloop.** Er is geen panic op gebruikersinvoer. De meldingen noemen de oorzaak en gebruiken `1 combination` of `N combinations`:

- `allotment does not fit: the number of hosts times the metrics per host is too large`
- `count does not fit: the sum of --histogram-aggregates A and --histogram-percentiles P is too large`
- `count does not fit: metric NAME: factor F times N combinations is too large`
- `count does not fit: metric NAME: factor F times N combinations makes the total too large`

De som van de histogram-opties is alleen een fout als een histogram in het sample die factor echt gebruikt. Een sample zonder histogram telt nog steeds als die vlaggen extreem groot zijn. Exitcode 3, met het voorvoegsel `usage error: `.

Elk tekstverslag eindigt met `estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)`.

## Invoerformaten

**DogStatsD.** `name:value|type`, daarna optioneel `|@rate` en `|#tag:value,tag` in beide volgordes. Een tag mag geen waarde hebben. Lege regels worden overgeslagen. Een waarde die geen eindig getal is, inclusief `NaN` en `Inf`, is ongeldig (exit 2). Een heel groot eindig getal telt nog steeds: de waarde hoort niet bij de rekening.

**JSON-regels.** Eén object per regel: `metric` (tekst), `type` (`count`, `gauge`, `histogram` of `distribution`) en `tags` (een lijst van `key:value` of `key`). Andere velden worden genegeerd. Lege regels worden genegeerd.

**Prometheus-tekst.** `# TYPE` zet de familie. `counter` en `gauge` tellen als D3. `histogram` en `summary` volgen D10, niet de DogStatsD-factor 5. `# TYPE untyped`, of een sample zonder `# TYPE`, telt als 0 en staat als `skipped` (D11), tenzij `--untyped-as` gezet is. Een `_count`-, `_sum`- of `_bucket`-naam waarvan de basis een counter of gauge is, is die aparte metric zonder type, geen histogram. Onbekende types zijn een fout. `# HELP`-regels worden genegeerd.

Een `_total`-sample van een counter-familie telt als `name.count` (D12). De OpenMetrics-pagina zegt: "counter names ending in `_total` must be specified without the `_total` suffix. For example, to collect `promhttp_metric_handler_requests_total`, specify the metric name `promhttp_metric_handler_requests`. This submits to Datadog the metric name appended with `.count`, `promhttp_metric_handler_requests.count`." (https://docs.datadoghq.com/integrations/openmetrics/, gelezen 7 oktober 2026). `# TYPE a_total counter` met een sample `a_total` blijft de metric `a_total` (D3), omdat de typenaam exact de samplenaam is. Een `_created`-sample van een counter-familie telt niet mee. Wat Datadog met `_created` doet is niet geverifieerd.

Een metricnaam mag een punt bevatten, zonder quotes. https://prometheus.io/docs/concepts/data_model/ (gelezen 9 oktober 2026) zegt "Metric names MAY use any UTF-8 characters." en "Metric names SHOULD match the regex `[a-zA-Z_:][a-zA-Z0-9_:]*` for the best experience and compatibility (see the warning below). Metric names outside of that set will require quoting e.g. when used in PromQL (see the UTF-8 guide)." De tool accepteert een punt zonder quotes, omdat Datadog-namen zoals `request.Latency` in bron B1 staan. De tekens die de parser in een naam accepteert zijn letters, cijfers, `_`, `:` en `.`.

Een labellijst mag op een komma eindigen. Het exposition-formaat schrijft `label_pairs = label_pair { "," label_pair } [ "," ]` (https://github.com/prometheus/docs/blob/main/docs/instrumenting/exposition_formats.md, gelezen 8 oktober 2026). Spaties rond `=` worden geaccepteerd, bijvoorbeeld `a{host = "x"} 1`. De EBNF op die pagina (gelezen 9 oktober 2026) schrijft de labelnaam, dan `=`, dan een gequote waarde, zonder spaties. Dezelfde pagina zegt dat tokens door spaties of tabs gescheiden mogen worden. Deze tool accepteert de spaties: milder dan de grammatica, nooit strenger.

`+Inf` is een geldige Prometheus-waarde. Een timestamp, als die er is, is een optioneel geheel getal. Een UTF-8 BOM aan het begin van een DogStatsD-, JSONL- of Prometheus-bestand wordt overgeslagen. Een BOM verderop in het bestand blijft deel van de tekst. Stuurtekens in een metricnaam of tagsleutel worden in tekst als `\xNN` getoond, zodat ze de terminal niet sturen. JSON houdt de ruwe string.

Getallen lopen via `serde_json` 1.0.151, dat afhangt van `zmij` 1.0.23 van David Tolnay (MIT). Die bibliotheek doet geen netwerk-, bestands- of procesaanroep. Het build-script draait `rustc --version` tijdens het compileren.

## JSON

`schema_version` is 1. `tool` is `dd-metric-bill`. `estimate` is de schattingsregel. `source` is de URL van custom-metrics billing. Bij `count` is `inputs` de lijst paden en `total` een getal. Bij `diff` is `inputs` `{"old","new"}` en `total` `{"old","new","delta"}`.

Elke metric heeft `name`, `type`, `combinations`, `count`, `rule` en `rule_text`. Met `--allow-tags` hebben metrics ook `ingested` en `indexed`, en het document heeft die totalen. `tags` noemt sleutels met `values` en `added`. `unbounded` noemt sleutels met `reasons`. `allotment` is `null` zonder `--plan`. Zonder allowlist heeft hij `usage`, `basis` (`total (indexed)`) en `surplus`. Met een allowlist heeft hij `indexed`, `indexed_surplus`, `ingested` en `ingested_surplus`, en geen sleutel `basis`. `costs` staat er alleen met `--prices`: `currency`, `source`, `date` en `lines` met `kind`, `above` en `amount`. `diff` laat `allotment` null en `unbounded` leeg, en laat `ingested`, `indexed` en `costs` weg.

`--explain` verandert de JSON niet: `rule_text` en `source` staan er altijd. Tagwaarden zitten er niet in.

## Exitcodes en gebruiksfouten

| Code | Betekenis |
| --- | --- |
| 0 | Het verslag is geschreven. Met `--budget` ligt het totaal niet boven N. |
| 1 | `--budget` overschreden. Stdout heeft nog steeds het verslag. Stderr is `budget exceeded: TOTAL > LIMIT`. |
| 2 | Het sample of het prijsbestand kan niet gelezen worden of is ongeldig. |
| 3 | Gebruiksfout. Stdout is leeg. |

Een sample en een prijsbestand moeten gewone bestanden zijn. `metadata` volgt symlinks. Een map, FIFO, device of ander niet-regulier pad wordt niet geopend, zodat een FIFO zonder schrijver niet blijft hangen. De melding is `cannot read sample: PATH: REASON` of `cannot read price file: PATH: REASON`, en `REASON` bevat `not a regular file` als het pad geen gewoon bestand is.

Een prijsbestand dat geen geldige JSON is, geen object is, of een slecht veld heeft, schrijft `invalid price file: PATH: DETAIL`. `DETAIL` is een van: `not valid json`, `not an object`, `missing field: FIELD`, `empty field: FIELD`, `field is not text: FIELD`, `field is not a number: FIELD`, `field is negative: FIELD`, `field is too large: FIELD`, `unknown field`.

Een slecht sample noemt het formaat en het regelnummer vanaf 1, nooit de regel zelf:

- `invalid dogstatsd line N: shape is wrong`
- `invalid dogstatsd line N: name is missing`
- `invalid dogstatsd line N: value is missing or invalid`
- `invalid dogstatsd line N: type is missing or unknown`
- `invalid dogstatsd line N: sample rate is missing or invalid`
- `invalid dogstatsd line N: tag is missing or invalid`
- `invalid dogstatsd line N: unknown field`
- `invalid jsonl line N: not valid json`
- `invalid jsonl line N: shape is wrong`
- `invalid jsonl line N: name is missing`
- `invalid jsonl line N: type is missing or unknown`
- `invalid jsonl line N: tag is missing or invalid`
- `invalid prometheus line N: shape is wrong`
- `invalid prometheus line N: name is missing or invalid`
- `invalid prometheus line N: type is missing or unknown`
- `invalid prometheus line N: type is duplicated`
- `invalid prometheus line N: label is missing or invalid`
- `invalid prometheus line N: value is missing or invalid`
- `invalid prometheus line N: histogram sample is missing _bucket, _sum or _count`

Bij `diff` worden verboden opties in deze volgorde gecontroleerd: `--explain`, `--plan`, `--hosts`, `--allow-tags`, `--prices`, `--unbounded-threshold`. De eerste wint, dus `--explain` wint van een latere vlag. `--json` verandert de melding niet. Een ongeldige waarde meldt de argumentparser vóór die controle.

Elke rij hieronder is tegen de binary gedraaid, en een test houdt dat zo. Een rij met exit 3 heeft lege stdout. De melding is de volledige stderr; de binary zet er een newline achter. `\n` in de laatste kolom is een newline in de melding.

| Optie | Geldt voor | Geval | Verkeerd gebruik | Exit | Melding |
| --- | --- | --- | --- | --- | --- |
| `--format` | count | Ongeldige waarde. | `count --format nope tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'nope' for '--format <FORMAT>'\n  [possible values: dogstatsd, jsonl, prometheus]\n\nFor more information, try '--help'.\n` |
| `--plan` | count | Ongeldige waarde. | `count --plan maybe tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'maybe' for '--plan <PLAN>'\n  [possible values: pro, enterprise]\n\nFor more information, try '--help'.\n` |
| `--untyped-as` | count | Ongeldige waarde. | `count --untyped-as nope tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'nope' for '--untyped-as <KIND>'\n  [possible values: gauge, count]\n\nFor more information, try '--help'.\n` |
| `--histogram-aggregates` | count | Geen geheel getal. | `count --histogram-aggregates nope tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'nope' for '--histogram-aggregates <N>': invalid digit found in string\n\nFor more information, try '--help'.\n` |
| `--top` | count | Geen geheel getal. | `count --top nope tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'nope' for '--top <N>': invalid digit found in string\n\nFor more information, try '--help'.\n` |
| `--budget` | count | Geen geheel getal. | `count --budget nope tests/fixtures/empty.dogstatsd` | 3 | `error: invalid value 'nope' for '--budget <N>': invalid digit found in string\n\nFor more information, try '--help'.\n` |
| `--hosts` | count | Zonder `--plan`. | `count --hosts 1 tests/fixtures/empty.dogstatsd` | 3 | `--hosts only works together with --plan` |
| `--untyped-as` | count | Zonder prometheus. | `count --untyped-as gauge tests/fixtures/empty.dogstatsd` | 3 | `--untyped-as only works together with --format prometheus` |
| `--explain` | diff | Niet voor diff. | `diff --explain tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --explain` |
| `--explain` | diff | `--json` verandert de melding niet. | `diff --json --explain tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --explain` |
| `--plan` | diff | Gecontroleerd vóór `--hosts`. | `diff --plan pro --hosts 1 tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --plan` |
| `--hosts` | diff | Niet voor diff. | `diff --hosts 1 tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --hosts` |
| `--allow-tags` | diff | Niet voor diff. | `diff --allow-tags host tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --allow-tags` |
| `--prices` | diff | Geweigerd voordat het prijsbestand wordt gelezen. | `diff --prices prices/datadog-example.json tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --prices` |
| `--unbounded-threshold` | diff | Op de opdrachtregel gezet, ook met de standaardwaarde. | `diff --unbounded-threshold 50 tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd` | 3 | `option does not apply to diff: --unbounded-threshold` |
| `--help` | hoofdniveau | Onbekende optie. | `--nope` | 3 | `error: unexpected argument '--nope' found\n\n  tip: a similar argument exists: '--top'\n\nUsage: dd-metric-bill --top <N> <COMMAND>\n\nFor more information, try '--help'.\n` |
| `diff` | diff | Het nieuwe sample ontbreekt. | `diff tests/fixtures/diff-old.dogstatsd` | 3 | `error: the following required arguments were not provided:\n  <NEW>\n\nUsage: dd-metric-bill diff <OLD> <NEW>\n\nFor more information, try '--help'.\n` |
