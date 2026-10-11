# dd-metric-bill

[English](README.md) · Nederlands

Schat offline hoeveel factureerbare Datadog custom metrics een sample oplevert, voordat de rekening komt.

[![crates.io](https://img.shields.io/crates/v/dd-metric-bill.svg)](https://crates.io/crates/dd-metric-bill) [![CI](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/workflows/ci.yml/badge.svg)](https://github.com/qwpp7g79y4-maker/dd-metric-bill/actions/workflows/ci.yml)

[Installatie](#installatie) • [Snel aan de slag](#snel-aan-de-slag) • [Hoe het rekent](#hoe-het-rekent) • [Alternatieven](#vergelijking-met-alternatieven) • [Beperkingen](#beperkingen) • [Referentie](docs/reference.nl.md)

dd-metric-bill is een opdrachtregelprogramma in Rust. Het leest één of meer samples op je eigen machine als DogStatsD-regels (`naam:waarde|type|#tag:waarde`), als JSON-regels of als Prometheus-tekst. Een custom metric is een unieke combinatie van metricnaam en tagwaarden, inclusief de `host`-tag. De volgorde van de tags telt niet. COUNT, RATE en GAUGE tellen als 1 per combinatie. Een histogram telt standaard 5 per combinatie: 4 aggregaties plus 1 percentiel. Een distribution telt 5 per combinatie, of 10 met `--percentiles`. Per tag-sleutel staat hoeveel unieke waarden die sleutel heeft en hoeveel series hij toevoegt. Onbegrensde tags (te veel waarden, of een patroon zoals een UUID) krijgen een reden. `--allow-tags` simuleert Metrics without Limits: ingested op alle tags, indexed op de allowlist. `diff` vergelijkt twee samples. `--budget N` geeft exitcode 1 als het totaal boven N ligt. `--plan` en `--hosts` tonen de allotment. `--prices` leest een prijsbestand en schat de kosten boven de allotment. `--json` schrijft dezelfde uitkomst als JSON. De standaarduitvoer toont sleutels en aantallen, niet de tagwaarden. Contracten met Metric Name pricing worden niet ondersteund. Alleen count accepteert --explain, --plan, --hosts, --allow-tags, --prices en --unbounded-threshold. --hosts werkt alleen samen met --plan. --untyped-as werkt alleen samen met --format prometheus. Het heeft geen account nodig en maakt geen netwerkverbinding.

De uitvoer van de tool is Engels; de voorbeelden hieronder tonen die precies zoals de binary hem schrijft.

## Waarom

Datadog rekent custom metrics af per unieke combinatie van metricnaam en tagwaarden. Eén tag met veel waarden, bijvoorbeeld `user_id`, kan het aantal metrics en dus de rekening sterk laten stijgen. Een team ziet dat pas achteraf op de Usage-pagina. Met dd-metric-bill ziet het team vooraf en lokaal welke metric en welke tag de kosten veroorzaakt.

## Installatie

Met Rust 1.85 of nieuwer installeer je vanaf [crates.io](https://crates.io/crates/dd-metric-bill):

```sh
cargo install --locked dd-metric-bill
dd-metric-bill --help
```

Kant-en-klare binaries voor Linux (x86_64), Windows (x86_64) en macOS (Intel en Apple Silicon), elk met een SHA-256-checksum, staan op de [Releases-pagina](https://github.com/qwpp7g79y4-maker/dd-metric-bill/releases).

Voor de nieuwste `main`: `cargo install --locked --git https://github.com/qwpp7g79y4-maker/dd-metric-bill`.

Vanuit een lokale clone: `cargo install --locked --path .`, of `cargo build --release`; de binary is `target/release/dd-metric-bill`.

## Snel aan de slag

Dit is de echte uitvoer van de binary op de synthetische fixtures in `tests/fixtures`. Een test draait elk voorbeeld en vergelijkt de uitvoer.

Het rekenvoorbeeld uit de Datadog-documentatie, als count. Vier unieke combinaties, dus totaal 4:

```console
$ dd-metric-bill count tests/fixtures/b1-count.dogstatsd
total: 4

request.Latency type: count combinations: 4 count: 4 rule: D3
  tag: endpoint values: 2 added: 1
  tag: host values: 2 added: 1
  tag: status values: 2 added: 1

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
```

Hetzelfde sample als histogram. Vier combinaties maal 5 is 20:

```console
$ dd-metric-bill count tests/fixtures/b1-histogram.dogstatsd
total: 20

request.Latency type: histogram combinations: 4 count: 20 rule: D4
  tag: endpoint values: 2 added: 1
  tag: host values: 2 added: 1
  tag: status values: 2 added: 1

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
```

Twee samples vergelijken. De tag `region` komt erbij, `checkout` is nieuw en `queue.depth` is weg. Het totaal gaat van 3 naar 4:

```console
$ dd-metric-bill diff tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd
old total: 3
new total: 4
delta: 1

new:
checkout type: count count: 1

removed:
queue.depth type: gauge count: 1

checkout type: count old: 0 new: 1 delta: 1 rule: D3
  tag: env values: 0 to 1 added: 0 to 0 delta: 0
page.views type: count old: 2 new: 3 delta: 1 rule: D3
  tag: region values: 0 to 2 added: 0 to 1 delta: 1
  tag: env values: 1 to 1 added: 0 to 0 delta: 0
  tag: host values: 2 to 2 added: 1 to 1 delta: 0
queue.depth type: gauge old: 1 new: 0 delta: -1 rule: D3
  tag: host values: 1 to 0 added: 0 to 0 delta: 0

estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
```

Een leeg sample heeft totaal 0. De schattingsregel staat er altijd:

```console
$ dd-metric-bill count tests/fixtures/empty.dogstatsd
total: 0
estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)
```

Prijzen komen alleen uit een JSON-bestand dat je met `--prices` meegeeft; zie [het prijsbestand](docs/reference.nl.md#prijsbestand). Elke tekstuitvoer eindigt met `estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)`. Alle opties, exitcodes en foutmeldingen staan in de [referentie](docs/reference.nl.md) en in `dd-metric-bill --help`.

## Hoe het rekent

Een custom metric is één combinatie van metricnaam en tagwaarden, inclusief `host`. De pagina van Datadog zegt `Reordering tag values doesn't add uniqueness` (https://docs.datadoghq.com/account_management/billing/custom_metrics, gelezen 7 oktober 2026). COUNT, RATE en GAUGE zijn 1 per combinatie. Een histogram is standaard 5 per combinatie (4 aggregaties plus `95percentile`), een distribution is 5, of 10 met percentielen. `--allow-tags endpoint,status` op het count-sample hierboven is ingested 4 en indexed 3; op het distribution-sample is het ingested 20 en indexed 15. De volledige regels, de Prometheus-mapping en de allotment staan in de [referentie](docs/reference.nl.md).

## Vergelijking met alternatieven

**nicholas-ruest/series-budget** is een beleidspoort met een TOML-beleid: budgetten per metric en per label, verboden en verplichte labels, en een allowlist per metric. Bevindingen krijgen regelcodes SB001-SB007. De tool leest alleen JSONL en geeft exit 2 bij bevindingen.

dd-metric-bill past de regels van Datadog toe. De `host`-tag telt mee, een histogram of distribution vermenigvuldigt per combinatie, en Prometheus volgt de mapping D10 en D11. De allotment hangt af van plan en aantal hosts. `--allow-tags` scheidt ingested van indexed. `--prices` leest een prijsbestand.

## Beperkingen

- Geen Metric Name pricing. Contracten met Metric Name pricing worden niet ondersteund.
- De uitkomst is een schatting. Datadog rekent het uurgemiddelde over de maand. Deze tool berekent geen uurgemiddelde: een sample is één momentopname. Elke tekstuitvoer eindigt met `estimate: Datadog bills the hourly average over the month; this is one snapshot (source: B1)`.
- Geen verbinding met Datadog. Er is geen API-key en geen live usage. Volledig offline: geen netwerk, geen telemetrie.
- De allowlist geldt voor alle metrics tegelijk. In Datadog is de allowlist per metric.
- Zonder `--hosts` wordt het aantal hosts afgeleid uit de `host`-tag. Datadog rekent met betaalde hosts.
- Alleen de "Latest Version"-mapping wordt gevolgd. De mapping komt uit B2 (Mapping Prometheus Metrics to Datadog Metrics, https://docs.datadoghq.com/integrations/guide/prometheus-metrics/, gelezen 7 oktober 2026). Het overslaan van untyped komt uit B3 (Datadog, OpenMetrics, sectie Missing untyped metrics, https://docs.datadoghq.com/integrations/openmetrics/, gelezen 7 oktober 2026): "By default, the integration skips metrics that come without a type on a Prometheus exposition." Dat is het standaardgedrag van de OpenMetrics-check van de Datadog Agent.
- Een UTF-8 BOM aan het begin van een bestand wordt overgeslagen, in DogStatsD, JSONL en Prometheus. Dat is een keuze: de BOM hoort niet bij de metricnaam. Een BOM verderop in het bestand blijft staan.
- Het `_created`-sample van een OpenMetrics-counter telt niet mee. Wat Datadog daarmee doet is niet geverifieerd.
- Alleen gewone bestanden worden gelezen, als sample bij `count` en `diff` en als `--prices`. Een map, FIFO, device of ander niet-regulier pad wordt niet geopend.
- Alleen count accepteert --explain, --plan, --hosts, --allow-tags, --prices en --unbounded-threshold. Bij `diff` geven ze exit 3 (`option does not apply to diff: <option>`), vóór het lezen van samples of het prijsbestand.
- --hosts werkt alleen samen met --plan. --untyped-as werkt alleen samen met --format prometheus.
- Er is geen `--show-values`-vlag. De uitvoer toont tag-sleutels en aantallen, niet de tagwaarden.
- Een prijs heeft hoogstens 12 cijfers achter de komma (`MAX_SCALE`) en een teller van hoogstens 10^18 (`MAX_NUMERATOR`). Nullen aan het eind van het decimale deel vallen weg vóór die grens. Een prijs daarbuiten maakt het prijsbestand ongeldig (exit 2, melding "field is too large").

## Steunen

dd-metric-bill is gratis te gebruiken. Bespaart het je geld, dan kun je het werk steunen via [Ko-fi](https://ko-fi.com/pepai).

## Licentie

Naar keuze onder een van deze licenties:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

Copyright 2026 PEPAI.

## Contact

PEPAI, pep_ai@icloud.com

Meld bugs en ideeën via [Issues](https://github.com/qwpp7g79y4-maker/dd-metric-bill/issues); meld beveiligingsproblemen privé zoals beschreven in [SECURITY.md](SECURITY.md).
