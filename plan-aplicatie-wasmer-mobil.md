# Plan: WasmDroid, un „Docker Desktop” pentru Wasmer pe Android

*Versiunea 4, 18 noiembrie 2026. Include deciziile pentru prima versiune Android (secțiunea 9).*

**Pe scurt:** WasmDroid e o aplicație gratuită și open-source pe Google Play, întâi pentru Android, cu UI nativ (Jetpack Compose). E făcută pentru utilizatori obișnuiți, care pornesc un pachet cu un tap; terminalul și logurile apar doar în „Mod avansat”. Docker local vine eventual mai târziu, dar arhitectura e pregătită de acum.

## 1. Ce construim

O aplicație mobilă care face pentru pachetele Wasmer ce face Docker Desktop pentru containere:

- caută și descarcă pachete din registry-ul Wasmer (wasmer.io);
- le pornește, le oprește, le repornește, le șterge;
- arată starea fiecărei instanțe, logurile (stdout/stderr), resursele folosite;
- gestionează volume (fișiere persistente), variabile de mediu, porturi;
- oferă un terminal pentru pachetele interactive (de ex. `python`, `bash`, `sqlite`).

**În afara etapei 1:** rularea containerelor Docker. Planul ia însă de acum deciziile de arhitectură (secțiunea 6) ca Docker să poată fi adăugat mai târziu **fără rescriere**.

---

## 2. Ce e realist tehnic (noiembrie 2026)

| Subiect | Android | iOS |
|---|---|---|
| Backend-uri Wasmer suportate | Toate: Singlepass, Cranelift, LLVM (compilatoare JIT/AOT), plus V8 | Doar V8 și „Browser” (WebKit). **Fără** Cranelift/LLVM/Singlepass |
| JIT în proces | Permis (memorie executabilă în propriul proces) | Interzis pentru aplicațiile din App Store; rămâne interpretare / V8 „jitless” / motorul Wasm din WKWebView |
| Performanță estimată | Aproape nativă (Cranelift/LLVM) | Mult mai lentă (interpretare), OK pentru unelte CLI, slabă pentru sarcini grele |
| SDK oficial | Crate-urile Rust `wasmer` + `wasmer-wasix` se compilează pentru `aarch64-linux-android` | **Wasmer Swift SDK** (anunțat 23 sept. 2026): rulează Python, Node.js, FFmpeg, Clang în iOS/macOS prin WebKit într-un WKWebView ascuns. Cere iOS 27+, e experimental, demo-ul nu a trecut încă prin App Review |
| Rulare în fundal | Foreground Service cu notificare, poate ține servere pornite ore întregi | Foarte limitat: procesele se suspendă la scurt timp după ieșirea din aplicație |
| Politici magazin | Google Play restricționează descărcarea de cod executabil, dar acceptă cod care rulează într-un sandbox/VM fără acces direct la API-urile Android. Wasm în sandbox se încadrează în mod rezonabil | Regula 2.5.2 (cod descărcat). Precedente acceptate: a-Shell (rulează comenzi Wasm), iSH, Pyto. Risc real de respingere, trebuie prezentat ca „mediu de dezvoltare/educațional” |

**Registry Wasmer:** există un API GraphQL public (`registry.wasmer.io/graphql`) pentru căutare, metadate și versiuni. Pachetele sunt în format **webc** (container Wasm cu mai multe module, fișiere și comenzi). Există și crate-urile Rust `wasmer-registry` / `wasmer-api`, pe care le putem refolosi.

**Concluzie:** Android oferă experiența completă. iOS e posibil, dar mai lent, mai limitat în fundal și cu risc la App Store.

---

## 3. Recomandări (platformă și stack)

### Platformă: Android întâi (decis), iOS fără dată
Pe Android avem JIT, rulare în fundal și o politică de magazin mai permisivă, deci putem valida produsul repede. iOS se reevaluează mai târziu (vezi secțiunea 8.1 pentru limitările fără JIT). Nucleul Rust rămâne portabil, ca drumul spre iOS să fie deschis.

### Stack: nucleu Rust comun + UI nativ subțire
- **Nucleu („engine”) în Rust**: registry, cache, runtime, rețea, volume, loguri, baza de date. Wasmer e scris în Rust, deci integrarea e directă și codul se refolosește 100% pe iOS și, ulterior, pe desktop.
- **Legătura cu UI-ul**: [UniFFI](https://github.com/mozilla/uniffi-rs) generează automat binding-uri Kotlin și Swift.
- **UI nativ (decis)**: Jetpack Compose pe Android, SwiftUI pe iOS când va fi cazul.
- **Stocare metadate**: SQLite (prin `rusqlite`) în nucleu.

> Ideea principală: UI-ul nu știe nimic despre Wasmer. Vorbește doar cu un API intern de tip „engine” (secțiunea 5). Asta face posibil Docker mai târziu.

---

## 4. Funcționalități pe etape

### Etapa 0: spike tehnic (2–3 săptămâni)
- Compilăm `wasmer` + `wasmer-wasix` pentru Android arm64 și rulăm `python/python` și un server HTTP simplu (de ex. `wasmer/static-web-server`) în aplicație.
- Măsurăm: dimensiunea APK-ului, timpul de compilare la prima pornire, memoria, viteza Cranelift vs LLVM vs Singlepass.
- Verificăm că socket-urile WASIX funcționează (un server ascultă pe `127.0.0.1:8080` și îl deschidem din browserul telefonului).
- **Criteriu de ieșire:** un pachet descărcat din registry rulează și răspunde la HTTP.

### Etapa 1: MVP Android pentru utilizatori obișnuiți (8–10 săptămâni)

**Principiu de design:** utilizatorul găsește pachetele în Store, descarcă o versiune și creează un serviciu pe telefon. Pachetele cu interfață web se deschid direct din aplicație. Setările tehnice se configurează în formularul serviciului.

- **Store**: Wasmer Registry este singura sursă de pachete afișate pentru căutare. Pachetele sunt publice. Store afișează avertismentul de compatibilitate înainte de descărcare și configurare.
- **Instalare cu un tap**: descărcare cu progres, cache adresat după conținut (sha256), spațiu ocupat, dezinstalare.
- **Creare serviciu**: din lista pachetelor descărcate, utilizatorul deschide formularul cu pachetul și o comandă precompletate. Salvarea creează serviciul; pornirea rămâne o acțiune explicită.
- **Pornire / oprire / repornire**; stări clare, în limbaj simplu („Pornește…”, „Rulează”, „Oprit”, „A dat eroare”) peste automatul tehnic de stări (vezi 6.4).
- **„Deschide”**: pachetele cu UI web se deschid într-un WebView cu profil separat pentru fiecare aplicație sau în browserul extern. Controlul complet al traficului WebView se amână.
- **Rulare în fundal** prin Foreground Service, cu notificare care arată câte aplicații rulează și buton „Oprește tot”.
- **Erori explicate** pe înțeles („nu mai e spațiu”, „pachetul nu e compatibil”), cu buton „Trimite raport”.
- **Resurse**: monitorizare RAM și CPU la nivelul procesului de serviciu și timeout configurabil. Un câmp timeout gol permite rularea fără limită de timp. Limitele stricte de RAM și CPU se amână.
- **Mod avansat** (activat din Setări, ascuns implicit): loguri live, terminal interactiv (TTY) și controale tehnice suplimentare.
- Cache cu modulele deja compilate, ca a doua pornire să fie instantanee.
- Aplicație gratuită, fără conturi; doar pachete publice.

### Etapa 2: mai mult control (4–6 săptămâni)
- Date ale aplicațiilor: backup/restaurare și import/export fișiere (prin Storage Access Framework). În modul avansat: volume numite.
- Pornire automată la boot și repornire la eroare (comutatoare simple; în spate, politici de restart `no` / `on-failure` / `always`).
- „Pachete combinate” (de ex. aplicație + bază de date) definite prin importul unui fișier „stack” (6.10), cu pachete din registry sau artefacte locale instalate.
- Partajare în rețeaua locală („Deschide de pe laptop”), cu cod QR; dezactivată implicit.
- Widget și Quick Settings tile pentru pornire și oprire rapidă.
- Mod avansat: import de pachete locale (`.webc` / `.wasm`).

### Etapa 3: iOS (fără dată; se reevaluează)
- Același nucleu Rust; backend WebKit/JavaScriptCore (Wasmer Swift SDK) cu interpretor ca rezervă (vezi 8.1).
- UI SwiftUI.
- Pregătim argumentația pentru App Review.

### Etapa 4: Docker local (doar dacă aplicația crește și apare nevoia; vezi secțiunea 7)

---

## 5. Arhitectura

```
┌──────────────────────────── UI (Compose / SwiftUI) ───────────────────────────┐
│  Catalog │ Pachete/Imagini │ Instanțe │ Volume │ Loguri/Terminal │ Setări     │
└───────────────────────────────────────┬───────────────────────────────────────┘
                                        │  Engine API (UniFFI, async + streams)
┌───────────────────────────────────────▼───────────────────────────────────────┐
│                               Engine (Rust)                                   │
│  ┌───────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐ │
│  │ Registry layer│ │ Content store│ │ State DB     │ │ Event bus + Log store│ │
│  │ (trait)       │ │ (sha256 blob)│ │ (SQLite)     │ │                      │ │
│  └──────┬────────┘ └──────────────┘ └──────────────┘ └──────────────────────┘ │
│         │          ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐ │
│         │          │ Volume mgr   │ │ Network mgr  │ │ Supervisor (restart, │ │
│         │          │              │ │ (porturi)    │ │  health, resurse)    │ │
│         │          └──────────────┘ └──────────────┘ └──────────────────────┘ │
│  ┌──────▼──────────────────────── Runtime drivers (trait) ───────────────────┐ │
│  │ wasmer-local  │ (viitor) docker-remote │ docker-vm │ container2wasm       │ │
│  └───────────────────────────────────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────────────────────┘
```

### Componente
- **Engine API**: singura interfață pentru UI. Operații după modelul Docker Engine API: `images.list/pull/remove`, `containers.create/start/stop/kill/remove/inspect/logs/attach`, `volumes.*`, `events.stream`. Folosim intenționat vocabularul Docker ca să nu-l redenumim mai târziu.
- **Registry layer**: trait `Registry` cu implementarea `WasmerRegistry` (GraphQL + descărcare webc). Mai târziu adăugăm `OciRegistry` (Docker Hub, GHCR) fără să modificăm restul.
- **Content store**: blob-uri adresate prin sha256, organizate după modelul OCI image layout. Un pachet webc e un blob plus un manifest. O imagine OCI va fi un set de layere plus un manifest. Același store le acceptă pe ambele.
- **State DB (SQLite)**: tabele `artifacts`, `instances`, `volumes`, `port_bindings`, `events`. Fiecare rând are coloanele `kind` și `driver` (vezi 6.1).
- **Supervisor**: pornește instanțele pe thread-uri sau task-uri separate, aplică politici de restart și timeout-ul opțional, colectează codul de ieșire și expune consumul RAM și CPU observat.
- **Log store**: buffer circular per instanță în memorie, plus fișier rotit pe disc. Suportă `follow`, `since`, `tail`.
- **Network manager**: alocă porturi pe host, ține evidența legăturilor `host:port → instanță:port` și detectează conflictele.
- **Volume manager**: directoare în spațiul privat al aplicației (`files/volumes/<nume>`), montate în WASIX ca „mapped dirs”.

### Izolare și securitate
- Fiecare instanță primește un sistem de fișiere virtual. Vede doar volumele montate explicit.
- Rețeaua e permisă doar pe loopback implicit. Accesul la internet și la LAN se acordă per instanță.
- Verificăm hash-ul fiecărui pachet descărcat. Afișăm editorul (namespace-ul) și avertizăm pentru pachetele neoficiale.

---

## 6. Decizii luate acum ca Docker să nu ceară rescriere

Acestea sunt deciziile obligatorii din etapa 1. Fiecare costă puțin acum și ar costa mult dacă ar fi introdusă ulterior.

### 6.1 Runtime-ul e un plugin („driver”), nu o presupunere
Definim trait-ul `RuntimeDriver` cu `create`, `start`, `stop(signal, timeout)`, `kill`, `wait`, `attach_stdio`, `stats`, `remove`. Wasmer e doar prima implementare (`wasmer-local`). Codul din afara driverului nu are voie să importe crate-uri `wasmer*`.

### 6.2 Model unificat de artefact („Image”)
```
Artifact {
  reference: "wasmer.io/python/python@3.12.0" | "docker.io/library/nginx:1.27"
  digest:    "sha256:…"
  kind:      Webc | Wasm | OciImage        // OciImage rezervat pentru viitor
  platform:  "wasm32-wasix" | "linux/arm64" | "linux/amd64"
  entrypoints/commands, size, labels
}
```
Referințele includ mereu registry-ul (`wasmer.io/…`), ca să coexiste cu `docker.io/…`. Folosim termenul „imagine” în cod și „pachet” în UI până la etapa Docker.

### 6.3 Specificația instanței e în format neutru, apropiat de OCI
```
InstanceSpec {
  artifact_ref, command, args, env, working_dir,
  mounts:   [{ volume | host_path, target, read_only }],
  ports:    [{ host_port, instance_port, protocol }],
  network:  { mode: loopback | lan | none | <rețea numită> },
  resources:{ memory_max, cpu_shares?, timeout? },
  restart_policy, labels, tty, stdin_open
}
```
Toate câmpurile au corespondent direct în `docker run`. Driverul Wasmer le traduce în opțiuni WASIX. Câmpurile pe care un driver nu le suportă sunt respinse explicit (`Unsupported`), nu ignorate în tăcere.

### 6.4 Ciclul de viață copiază stările Docker
`created → running → (paused) → exited | dead`, plus `restarting`. Același automat de stări pentru toate driverele, deci UI-ul nu se schimbă.

### 6.5 Stocare adresată după conținut, în stil OCI
Blob-urile se păstrează în `store/blobs/sha256/…`, cu manifeste și index. Un layer de imagine Docker intră în același loc. Garbage collection-ul pornește de la referințe, nu de la tipul de pachet.

### 6.6 Volumele sunt entități de sine stătătoare
Volumele au nume, nu sunt „folderul pachetului X”. Astfel același volum poate fi montat azi într-o instanță Wasm și mâine într-un container Docker.

### 6.7 Rețelistica se face prin port-mapping, nu prin acces direct
Instanțele nu „posedă” porturi de pe telefon. Network manager-ul le alocă. Când va exista o VM Linux, aceeași tabelă de mapări va face forwarding spre VM.

### 6.8 Engine API-ul e serializabil și poate fi folosit la distanță
Toate tipurile din Engine API sunt serializabile (serde). Pe lângă apelul in-process, se pot expune prin IPC sau HTTP. Asta permite ca aplicația să administreze mai târziu și un **engine Docker la distanță** (server, Raspberry Pi), lucru esențial pentru iOS.

### 6.9 Host-ul („Endpoint”) e un concept din prima zi
UI-ul lucrează cu un `Endpoint` (implicit: „Acest telefon”). Docker Desktop are „contexts”. Noi avem endpoint-uri: `local`, iar mai târziu `ssh://…`, `tcp://…`. Costă un câmp în plus azi.

### 6.10 Fișierul stack e compatibil cu un subset din Compose
Formatul fișierului multi-pachet (etapa 2) este un subset valid din `compose.yaml` (`services`, `image`, `command`, `environment`, `ports`, `volumes`, `depends_on`). Pentru pachete Wasm, `image: wasmer.io/…`.

### 6.11 Procesul engine-ului e separat de UI
Pe Android, engine-ul rulează în Foreground Service (eventual în proces separat, `android:process=":engine"`). O VM sau un proces greu adăugat ulterior are deja „casa” lui, iar UI-ul poate fi închis fără să oprească instanțele.

---

## 7. Cum poate fi adăugat Docker mai târziu (variante)

**Decizie:** ținta e **execuție locală**, nu un server la distanță. Docker nu intră în lucru în lunile următoare; îl reevaluăm doar după ce aplicația Wasmer-only crește. Până atunci se aplică doar deciziile din secțiunea 6. Când vom ajunge acolo, variantele locale realiste sunt B (unde AVF e disponibil), cu C sau D ca rezervă. Varianta A rămâne posibilă gratuit datorită 6.8–6.9, dar nu e prioritară.

| Variantă | Android | iOS | Comentariu |
|---|---|---|---|
| **A. Docker la distanță** (driver `docker-remote`, Docker Engine API prin SSH/TCP) | Da | Da | Cel mai simplu, funcționează oriunde. Aplicația devine „Docker Desktop” pentru un server propriu. **Recomandată ca primă variantă.** |
| **B. VM Linux locală prin Android Virtualization Framework** (AVF/pKVM, aceeași tehnologie ca aplicația „Terminal” Linux din Android 15+) | Doar pe dispozitive compatibile (Pixel și câteva altele); accesul la AVF pentru aplicații terțe e restricționat, de verificat | Nu | Performanță bună, dar disponibilitate limitată |
| **C. VM emulată (QEMU TCG)** cu un Linux minim + containerd | Da (lent) | Teoretic da, foarte lent (fără JIT) | Funcționează peste tot, dar consumă multă baterie. Potrivită pentru demo-uri, nu pentru uz serios |
| **D. container2wasm** (conversie imagine → Wasm cu emulator inclus) | Da | Da | Rulează prin același driver Wasmer. Foarte lent, dar e o punte elegantă pentru imagini mici |
| **E. „rootless” prin proot/chroot** (ca Termux) | Limitat (restricții W^X pe `exec` din Android 10+) | Nu | Fragil, nu-l recomandăm |

Datorită deciziilor din secțiunea 6, fiecare variantă înseamnă un **driver nou** plus `OciRegistry`. UI-ul, baza de date, volumele și logurile rămân neschimbate.

**De verificat înainte de etapa 4:**
- dacă aplicațiile terțe din Google Play pot folosi AVF (în noiembrie 2026 accesul pare rezervat aplicațiilor de sistem);
- ce procent din telefoanele utilizatorilor noștri au pKVM;
- dacă Google Play acceptă o aplicație care descarcă imagini Linux complete.

Pentru un public de utilizatori obișnuiți, fluxul Docker local va urmări aceeași simplitate ca fluxul Wasm. Sursa imaginilor Docker se decide în etapa 4.

---

## 8. Riscuri principale

1. **Politica Google Play privind codul descărcat** (Device and Network Abuse). Fiind singurul canal de distribuție, e riscul principal. *Atenuare:* Wasm rulează într-un sandbox fără acces la API-urile Android; descriere clară în Play Console; validăm local un APK intern și solicităm separat verdictul pentru distribuția prin Google Play.
2. **Compatibilitate variabilă în Wasmer Registry.** Pachetele publice pot folosi formate, comenzi sau facilități nesuportate. *Atenuare:* afișăm avertismentul de compatibilitate, validăm versiunea descărcată și verificăm exemple reale pe telefon.
3. **iOS fără JIT** (vezi 8.1). Nu blochează acum, fiindcă iOS e amânat.
4. **Compatibilitatea WASIX pe Android** (socket-uri, thread-uri, semnale). *Atenuare:* etapa 0 testează exact asta.
5. **Dimensiunea aplicației** (LLVM e mare). *Atenuare:* livrăm doar Cranelift (+ Singlepass), LLVM opțional.
6. **Consum de baterie și „kill” agresiv** de la producători (Xiaomi, Samsung). *Atenuare:* Foreground Service, ghid pentru setările de optimizare a bateriei, restart automat.
7. **Schimbări în API-ul registry-ului Wasmer.** *Atenuare:* izolare în `WasmerRegistry`, folosirea crate-urilor oficiale.
8. **Costuri de operare pentru o aplicație gratuită.** Store folosește serviciul public Wasmer Registry. Rapoartele sunt trimise explicit de utilizator prin formularul de proiect.

### 8.1 iOS fără JIT: dezavantaje și soluții
**Dezavantaje:**
- interpretoarele sunt de ~3–10× mai lente decât JIT (cele simple, 20–50×);
- consum mai mare de baterie;
- unele funcții Wasm (thread-uri, excepții, SIMD) lipsesc pe unele backend-uri;
- V8 „jitless” are Wasm-ul practic dezactivat;
- același pachet merge diferit pe Android și pe iOS.

**Soluții:**
1. **WKWebView/JavaScriptCore**: Apple permite JIT în procesul său WebContent. Wasmer Swift SDK folosește această abordare. Viteza e aproape de JIT, dar apelurile de sistem trec printr-o punte JS↔nativ, socket-urile de server trebuie emulate, aplicația nu rulează în fundal și cere iOS 27+. **Varianta recomandată.**
2. **Interpretor rapid** (WAMR fast-interp, wasmi), ca rezervă pentru unelte mici.
3. **AOT la build** pentru câteva pachete incluse în aplicație (doar pentru acestea, nu pentru cele descărcate).
4. **Execuție la distanță** prin endpoint-uri (6.8–6.9).
5. **Sideload cu JIT**: doar pentru pasionați, nu pentru produs.

---

## 9. Decizii luate (16–18 nov. 2026)

| # | Întrebare | Decizie |
|---|---|---|
| 1 | Platformă | Android întâi; iOS se decide mai târziu |
| 2 | UI | Nativ (Jetpack Compose; SwiftUI pentru iOS) |
| 3 | Public | Utilizatori obișnuiți; terminal și loguri ca opțiuni avansate, activate din Setări |
| 4 | Distribuție | Google Play |
| 5 | Model de business | Gratuit |
| 6 | Docker | Execuție locală, dar nu în lunile următoare; se reevaluează după ce aplicația Wasmer-only crește |
| 7 | Conturi / pachete private | Nu, în etapa 1 |
| 8 | Open-source | Da |
| 9 | Nume | **WasmDroid**; doar Android în prima etapă |
| 10 | Licență proiect | **Apache-2.0** |
| 11 | Sursa Store | Exclusiv Wasmer Registry. |
| 12 | Resurse în prima versiune | RAM/CPU observat și timeout opțional, inclusiv rulare fără limită de timp. Limitele stricte se amână. |
| 13 | WebView în prima versiune | Profil separat pentru fiecare aplicație. Controlul complet al traficului se amână. |
| 14 | Livrabil intern | APK Android cu runtime activat și acceptanță pe telefonul conectat. |

---

## 10. Pașii următori concreți

1. Conectăm Store în modul normal la fluxul Wasmer Registry de căutare, descărcare și creare a serviciului.
2. Compilăm APK-ul intern cu runtime activat și îl verificăm pe telefonul conectat, inclusiv cu pachete reale.
3. Înregistrăm comenzi, revizie, ABI, rezultate și limitări în `docs/verification`. Evaluarea separată de distribuție prin Google Play rămâne pentru etapa de lansare.

---

### Surse
- [Wasmer runtime features (backend-uri și platforme)](https://docs.wasmer.io/runtime/features)
- [Introducing Wasmer v5 (suport iOS, backend-uri noi)](https://wasmer.io/posts/introducing-wasmer-v5)
- [Wasmer Swift SDK pentru iOS/macOS](https://runtimewire.com/article/wasmer-swift-sdk-ios-macos)
- [Wasmer GraphQL API](https://docs.wasmer.io/graphql-api)
- [crate wasmer-registry](https://docs.rs/wasmer-registry)
