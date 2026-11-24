# VN Launcher

Launcher Minecraft: Java Edition mã nguồn mở (MIT), viết bằng **Rust** (`eframe`/`egui`), giao diện tiếng Việt, tông trắng – xanh lá.

## Tính năng

- **Vanilla chạy độc lập** – launcher không tự cài mod. Fabric, mod tối ưu, Distant Horizons, shader và resource pack đều là tuỳ chọn.
- **Quản lý phiên bản**: tạo, đổi tên, đổi phiên bản/loader, xoá (chỉ xoá thư mục `instances/<id>` của phiên bản đó, giữ `shared/`, `runtimes/` và tài khoản).
- **RAM theo máy**: tự đề xuất RAM dựa trên RAM thật của máy (máy 8 GB → 3 GB), cảnh báo khi cấp quá nhiều. JVM dùng G1GC; ZGC là tuỳ chọn (Java 21+).
- **Gói tăng FPS (tuỳ chọn)**: Fabric API, Sodium, Lithium, FerriteCore, ImmediatelyFast, EntityCulling, Mod Menu – cài từ Modrinth khi người dùng bấm.
- **Distant Horizons (tuỳ chọn)**: chọn tầm nhìn LOD 128/256/512/1024. Đây là tầm nhìn LOD của DH, *không phải* render distance vanilla (tối đa 32). Tầm nhìn càng xa càng tốn CPU/RAM; máy 8 GB nên dùng 256–512.
- **Mod / Resource pack / Shader**: tìm và cài từ Modrinth (tự kéo dependency bắt buộc), bật/tắt/xoá, kéo-thả file.
- **Tài khoản**: offline (UUID v3 chuẩn) và Microsoft (Device Code flow – cần Client ID riêng, xem bên dưới). Tài khoản lưu ở `%APPDATA%\VNLauncher\accounts.json`, token không ghi vào log.
- **Nhật ký game** ngay trong launcher, **nút DỪNG GAME** chỉ dừng đúng tiến trình Java do launcher mở.
- **Chống chạy trùng**: nút CHƠI bị khoá trong suốt lúc chuẩn bị và khi game còn chạy (theo dõi tiến trình thật).
- Tự tải Java (Adoptium 21/17/8) đúng bản Mojang yêu cầu; chế độ portable (`vn_launcher_data` cạnh file exe).

## Đăng nhập Microsoft

Launcher không kèm Client ID của bên thứ ba. Để dùng tài khoản premium:

1. Tạo App registration trên Azure Portal, loại tài khoản **Personal Microsoft accounts**, bật **Allow public client flows**.
2. Gửi Client ID cho Mojang duyệt tại https://aka.ms/mce-reviewappid (chưa được duyệt sẽ bị lỗi 403 khi đăng nhập Minecraft).
3. Dán Client ID vào tab **Tài khoản**, bấm đăng nhập và nhập mã tại microsoft.com/link.

## Build (Windows, toolchain GNU)

```powershell
rustup default stable-x86_64-pc-windows-gnu
rustup component add llvm-tools
cargo build --release
```

Lưu ý môi trường không có MSVC/MinGW đầy đủ đã dùng: `llvm-ar.exe` copy thành `dlltool.exe` trong `rustlib\x86_64-pc-windows-gnu\bin`, và `rust-lld.exe` copy thành `self-contained\ld.exe` (GNU BFD ld với import lib của llvm-dlltool tạo exe bị crash 0xC0000005).

File chạy: `target/release/vn-launcher.exe`.

## Mã nguồn

- `src/main.rs` – giao diện, luồng nền, quản lý tiến trình game
- `src/config.rs` – cấu hình, phiên bản, migration, RAM hệ thống
- `src/auth.rs` – tài khoản offline / Microsoft
- `src/mojang.rs`, `src/fabric.rs` – tải game, thư viện, natives, Fabric
- `src/java.rs` – tải Java portable
- `src/modrinth.rs` – Modrinth API
- `src/optimizer.rs` – tham số JVM, options.txt mặc định, chỉnh DistantHorizons.toml
- `src/launcher.rs` – chuẩn bị và khởi chạy game

## Giấy phép

MIT License.
