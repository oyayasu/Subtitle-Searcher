# Subtitle Searcher

A local desktop subtitle search tool for studying natural English through shows and movies you already watch.

## Library structure

Place a `Shows` folder next to the application:

```text
Subtitle Searcher/
├── Subtitle Searcher.exe
└── Shows/
    ├── Malcolm in the Middle/
    │   ├── S01E01 - Pilot.srt
    │   ├── S01E02 - Red Dress.srt
    │   └── Season 02/
    │       └── S02E01.srt
    ├── BoJack Horseman/
    │   └── S01E01.vtt
    └── The Truman Show/
        └── The Truman Show.srt
```

Each first-level folder inside `Shows` becomes one item in the app library. Subtitle files may be nested inside season folders.

## Current search behavior

The search is deliberately not exact-match-only. Results are ranked using:

- exact phrase matches;
- normalized punctuation/case;
- common English inflections and irregular verbs;
- token order and phrase coverage;
- common synonyms such as `trash/garbage`, `couch/sofa`, and `fridge/refrigerator`;
- fuzzy spelling similarity.

The UI groups matches as Exact, Very close, Close, or Related.

A semantic embedding layer is planned on top of this local ranking engine so paraphrases with little shared vocabulary can also be found reliably.

## Development

Requirements:

- Node.js 20+
- Rust stable
- Tauri 2 system prerequisites

```bash
npm install
npm run tauri dev
```

## Supported subtitles

- SubRip (`.srt`)
- WebVTT (`.vtt`)
