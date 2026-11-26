using System;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Forms;
using Microsoft.Win32;

[assembly: AssemblyTitle("VN Launcher Setup")]
[assembly: AssemblyDescription("Trình cài đặt VN Launcher 2.0 (Minecraft: Java Edition)")]
[assembly: AssemblyCompany("Nguyễn Trọng Việt Anh (Vanh)")]
[assembly: AssemblyProduct("VN Launcher")]
[assembly: AssemblyCopyright("Copyright © 2026 Nguyễn Trọng Việt Anh (Vanh)")]
[assembly: AssemblyVersion("2.1.2.0")]
[assembly: AssemblyFileVersion("2.1.2.0")]

namespace VNLauncherSetup
{
    public class SetupWizard : Form
    {
        [DllImport("user32.dll")]
        private static extern bool SetProcessDPIAware();

        private int pageIndex = 0;
        private Panel pageWelcome;
        private Panel pageLocation;
        private Panel pageInstalling;
        private Panel pageFinish;

        private TextBox txtInstallDir;
        private CheckBox chkDesktop;
        private CheckBox chkStartMenu;
        private CheckBox chkLaunchNow;

        private Label lblProgressStatus;
        private ProgressBar progressBar;
        private ListBox lstLog;

        private Button btnBack;
        private Button btnNext;
        private Button btnCancel;

        private string installedExePath = "";

        public SetupWizard()
        {
            this.Text = "Trình cài đặt — VN Launcher 2.0";
            this.ClientSize = new Size(520, 375);
            this.FormBorderStyle = FormBorderStyle.FixedDialog;
            this.MaximizeBox = false;
            this.StartPosition = FormStartPosition.CenterScreen;
            this.BackColor = Color.FromArgb(240, 240, 240);
            this.Font = new Font("Segoe UI", 9.0f, FontStyle.Regular);

            try
            {
                this.Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath);
            }
            catch { }

            string defaultDir = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
                ".vn_launcher"
            );

            // Bottom Wizard Bar
            Panel bottomBar = new Panel();
            bottomBar.Dock = DockStyle.Bottom;
            bottomBar.Height = 50;
            bottomBar.BackColor = Color.FromArgb(240, 240, 240);
            bottomBar.Paint += (s, e) =>
            {
                e.Graphics.DrawLine(new Pen(Color.FromArgb(208, 208, 208)), 0, 0, bottomBar.Width, 0);
                e.Graphics.DrawLine(Pens.White, 0, 1, bottomBar.Width, 1);
            };

            btnBack = new Button();
            btnBack.Text = "< Quay lại";
            btnBack.Size = new Size(88, 26);
            btnBack.Location = new Point(236, 13);
            btnBack.UseVisualStyleBackColor = true;
            btnBack.Click += (s, e) => ShowPage(pageIndex - 1);

            btnNext = new Button();
            btnNext.Text = "Tiếp tục >";
            btnNext.Size = new Size(88, 26);
            btnNext.Location = new Point(328, 13);
            btnNext.UseVisualStyleBackColor = true;
            btnNext.Click += OnNextClicked;

            btnCancel = new Button();
            btnCancel.Text = "Huỷ";
            btnCancel.Size = new Size(80, 26);
            btnCancel.Location = new Point(426, 13);
            btnCancel.UseVisualStyleBackColor = true;
            btnCancel.Click += (s, e) => this.Close();

            bottomBar.Controls.Add(btnBack);
            bottomBar.Controls.Add(btnNext);
            bottomBar.Controls.Add(btnCancel);
            this.Controls.Add(bottomBar);

            // Page 0: Welcome
            pageWelcome = CreateSidebarPage(
                "Chào mừng đến với Trình cài đặt VN Launcher",
                "Trình cài đặt này sẽ cài đặt VN Launcher 2.0 (Minecraft: Java Edition Launcher) do Nguyễn Trọng Việt Anh (Vanh) phát triển.\r\n\r\n" +
                "• Giao diện Tiếng Việt, mã nguồn mở (MIT)\r\n" +
                "• Tích hợp sẵn bộ 22 Mod Tối Ưu Ultra FPS & mở khóa 1024 đoạn tuỳ chọn\r\n" +
                "• Tự động tải Java & quản lý đa phiên bản độc lập\r\n\r\n" +
                "Nhấn \"Tiếp tục >\" để chọn vị trí cài đặt, hoặc \"Huỷ\" để thoát.",
                false
            );

            // Page 1: Location & Shortcuts
            pageLocation = CreateHeaderPage(
                "Chọn vị trí cài đặt",
                "Chọn thư mục cài đặt VN Launcher và các lối tắt tiện ích."
            );

            Label lblDirIntro = new Label();
            lblDirIntro.Text = "Trình cài đặt sẽ cài đặt VN Launcher vào thư mục dưới đây (mặc định trong AppData).\r\nĐể cài vào thư mục khác, nhấn \"Duyệt...\".";
            lblDirIntro.Location = new Point(24, 76);
            lblDirIntro.Size = new Size(470, 38);
            pageLocation.Controls.Add(lblDirIntro);

            txtInstallDir = new TextBox();
            txtInstallDir.Text = defaultDir;
            txtInstallDir.Location = new Point(24, 120);
            txtInstallDir.Size = new Size(375, 24);
            pageLocation.Controls.Add(txtInstallDir);

            Button btnBrowse = new Button();
            btnBrowse.Text = "Duyệt...";
            btnBrowse.Location = new Point(407, 118);
            btnBrowse.Size = new Size(85, 26);
            btnBrowse.UseVisualStyleBackColor = true;
            btnBrowse.Click += (s, e) =>
            {
                using (FolderBrowserDialog fbd = new FolderBrowserDialog())
                {
                    fbd.Description = "Chọn thư mục cài đặt VN Launcher:";
                    fbd.SelectedPath = txtInstallDir.Text;
                    if (fbd.ShowDialog(this) == DialogResult.OK)
                    {
                        txtInstallDir.Text = fbd.SelectedPath;
                    }
                }
            };
            pageLocation.Controls.Add(btnBrowse);

            GroupBox grpShortcuts = new GroupBox();
            grpShortcuts.Text = "Lối tắt & Tích hợp Windows";
            grpShortcuts.Location = new Point(24, 158);
            grpShortcuts.Size = new Size(468, 92);

            chkDesktop = new CheckBox();
            chkDesktop.Text = "Tạo biểu tượng VN Launcher trên màn hình nền (Desktop)";
            chkDesktop.Checked = true;
            chkDesktop.Location = new Point(16, 26);
            chkDesktop.Size = new Size(430, 22);
            grpShortcuts.Controls.Add(chkDesktop);

            chkStartMenu = new CheckBox();
            chkStartMenu.Text = "Tạo lối tắt trong Start Menu & Tìm kiếm Windows (Win + S)";
            chkStartMenu.Checked = true;
            chkStartMenu.Location = new Point(16, 54);
            chkStartMenu.Size = new Size(430, 22);
            grpShortcuts.Controls.Add(chkStartMenu);

            pageLocation.Controls.Add(grpShortcuts);

            Label lblSpace = new Label();
            lblSpace.Text = "Dung lượng ổ đĩa cần thiết: ~6.8 MB (Cấu hình mặc định có thể đổi lại trong mục Cài đặt của App).";
            lblSpace.ForeColor = Color.FromArgb(90, 90, 90);
            lblSpace.Location = new Point(24, 266);
            lblSpace.Size = new Size(470, 22);
            pageLocation.Controls.Add(lblSpace);

            // Page 2: Installing
            pageInstalling = CreateHeaderPage(
                "Đang cài đặt VN Launcher",
                "Vui lòng chờ trong khi VN Launcher đang được cài đặt lên máy tính của bạn."
            );

            lblProgressStatus = new Label();
            lblProgressStatus.Text = "Đang chuẩn bị cài đặt...";
            lblProgressStatus.Location = new Point(24, 76);
            lblProgressStatus.Size = new Size(468, 20);
            pageInstalling.Controls.Add(lblProgressStatus);

            progressBar = new ProgressBar();
            progressBar.Location = new Point(24, 100);
            progressBar.Size = new Size(468, 22);
            progressBar.Minimum = 0;
            progressBar.Maximum = 100;
            progressBar.Value = 0;
            pageInstalling.Controls.Add(progressBar);

            lstLog = new ListBox();
            lstLog.Location = new Point(24, 132);
            lstLog.Size = new Size(468, 168);
            lstLog.Font = new Font("Consolas", 8.5f);
            pageInstalling.Controls.Add(lstLog);

            // Page 3: Finish
            pageFinish = CreateSidebarPage(
                "Hoàn tất cài đặt VN Launcher",
                "Trình cài đặt đã cài đặt xong VN Launcher 2.0 lên máy tính của bạn.\r\n\r\n" +
                "Bạn có thể mở ứng dụng bất cứ lúc nào từ biểu tượng ngoài Desktop hoặc gõ \"VN Launcher\" trong thanh tìm kiếm Windows (Start Menu).\r\n\r\n" +
                "Nhấn \"Hoàn tất\" để đóng trình cài đặt.",
                true
            );

            this.Controls.Add(pageWelcome);
            this.Controls.Add(pageLocation);
            this.Controls.Add(pageInstalling);
            this.Controls.Add(pageFinish);

            ShowPage(0);
        }

        private Panel CreateSidebarPage(string title, string body, bool isFinish)
        {
            Panel p = new Panel();
            p.Location = new Point(0, 0);
            p.Size = new Size(520, 325);
            p.BackColor = Color.White;

            Panel sidebar = new Panel();
            sidebar.Location = new Point(0, 0);
            sidebar.Size = new Size(164, 325);
            sidebar.Paint += (s, e) =>
            {
                e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
                using (LinearGradientBrush br = new LinearGradientBrush(
                    sidebar.ClientRectangle,
                    Color.FromArgb(16, 110, 52),
                    Color.FromArgb(14, 62, 32),
                    LinearGradientMode.Vertical))
                {
                    e.Graphics.FillRectangle(br, sidebar.ClientRectangle);
                }
                DrawVnBadge(e.Graphics, 42, 34, 80);
                using (Font f = new Font("Segoe UI", 10.5f, FontStyle.Bold))
                using (SolidBrush sb = new SolidBrush(Color.FromArgb(220, 252, 231)))
                {
                    StringFormat sf = new StringFormat { Alignment = StringAlignment.Center };
                    e.Graphics.DrawString("VN LAUNCHER", f, sb, new RectangleF(0, 126, 164, 24), sf);
                }
                using (Font f2 = new Font("Segoe UI", 8.5f, FontStyle.Regular))
                using (SolidBrush sb2 = new SolidBrush(Color.FromArgb(134, 239, 172)))
                {
                    StringFormat sf = new StringFormat { Alignment = StringAlignment.Center };
                    e.Graphics.DrawString("Version 2.0 · Rust Edition", f2, sb2, new RectangleF(0, 148, 164, 20), sf);
                }
            };
            p.Controls.Add(sidebar);

            Label lblTitle = new Label();
            lblTitle.Text = title;
            lblTitle.Font = new Font("Segoe UI", 13.0f, FontStyle.Bold);
            lblTitle.ForeColor = Color.FromArgb(17, 38, 29);
            lblTitle.Location = new Point(186, 22);
            lblTitle.Size = new Size(314, 52);
            p.Controls.Add(lblTitle);

            Label lblBody = new Label();
            lblBody.Text = body;
            lblBody.Location = new Point(186, 82);
            lblBody.Size = new Size(314, 165);
            p.Controls.Add(lblBody);

            if (isFinish)
            {
                chkLaunchNow = new CheckBox();
                chkLaunchNow.Text = "Khởi chạy VN Launcher ngay bây giờ";
                chkLaunchNow.Font = new Font("Segoe UI", 9.5f, FontStyle.Bold);
                chkLaunchNow.ForeColor = Color.FromArgb(21, 128, 61);
                chkLaunchNow.Checked = true;
                chkLaunchNow.Location = new Point(188, 255);
                chkLaunchNow.Size = new Size(300, 26);
                p.Controls.Add(chkLaunchNow);
            }

            return p;
        }

        private Panel CreateHeaderPage(string title, string subtitle)
        {
            Panel p = new Panel();
            p.Location = new Point(0, 0);
            p.Size = new Size(520, 325);
            p.BackColor = Color.FromArgb(245, 247, 246);

            Panel header = new Panel();
            header.Location = new Point(0, 0);
            header.Size = new Size(520, 60);
            header.BackColor = Color.White;
            header.Paint += (s, e) =>
            {
                e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
                e.Graphics.DrawLine(new Pen(Color.FromArgb(215, 225, 218)), 0, 59, 520, 59);
                DrawVnBadge(e.Graphics, 462, 10, 40);
            };

            Label lblTitle = new Label();
            lblTitle.Text = title;
            lblTitle.Font = new Font("Segoe UI", 10.0f, FontStyle.Bold);
            lblTitle.ForeColor = Color.FromArgb(17, 38, 29);
            lblTitle.Location = new Point(18, 10);
            lblTitle.Size = new Size(430, 20);
            header.Controls.Add(lblTitle);

            Label lblSub = new Label();
            lblSub.Text = subtitle;
            lblSub.Location = new Point(30, 31);
            lblSub.Size = new Size(420, 20);
            lblSub.ForeColor = Color.FromArgb(80, 96, 88);
            header.Controls.Add(lblSub);

            p.Controls.Add(header);
            return p;
        }

        private static void DrawVnBadge(Graphics g, int x, int y, int size)
        {
            Rectangle rect = new Rectangle(x, y, size, size);
            using (GraphicsPath path = RoundedRect(rect, size / 4))
            using (LinearGradientBrush bg = new LinearGradientBrush(rect, Color.FromArgb(34, 197, 94), Color.FromArgb(20, 83, 45), LinearGradientMode.Vertical))
            using (Pen borderPen = new Pen(Color.FromArgb(134, 239, 172), Math.Max(1.5f, size / 24f)))
            {
                g.FillPath(bg, path);
                g.DrawPath(borderPen, path);
            }
            using (Font f = new Font("Segoe UI", size * 0.38f, FontStyle.Bold, GraphicsUnit.Pixel))
            using (SolidBrush sb = new SolidBrush(Color.White))
            {
                StringFormat sf = new StringFormat { Alignment = StringAlignment.Center, LineAlignment = StringAlignment.Center };
                g.DrawString("VN", f, sb, new RectangleF(x, y + 1, size, size), sf);
            }
        }

        private static GraphicsPath RoundedRect(Rectangle b, int r)
        {
            int d = r * 2;
            GraphicsPath path = new GraphicsPath();
            path.AddArc(b.X, b.Y, d, d, 180, 90);
            path.AddArc(b.Right - d, b.Y, d, d, 270, 90);
            path.AddArc(b.Right - d, b.Bottom - d, d, d, 0, 90);
            path.AddArc(b.X, b.Bottom - d, d, d, 90, 90);
            path.CloseFigure();
            return path;
        }

        private void ShowPage(int idx)
        {
            pageIndex = idx;
            pageWelcome.Visible = (idx == 0);
            pageLocation.Visible = (idx == 1);
            pageInstalling.Visible = (idx == 2);
            pageFinish.Visible = (idx == 3);

            if (idx == 0)
            {
                btnBack.Enabled = false;
                btnNext.Text = "Tiếp tục >";
                btnNext.Enabled = true;
                btnCancel.Enabled = true;
            }
            else if (idx == 1)
            {
                btnBack.Enabled = true;
                btnNext.Text = "Cài đặt";
                btnNext.Enabled = true;
                btnCancel.Enabled = true;
            }
            else if (idx == 2)
            {
                btnBack.Enabled = false;
                btnNext.Enabled = false;
                btnCancel.Enabled = false;
            }
            else if (idx == 3)
            {
                btnBack.Enabled = false;
                btnNext.Text = "Hoàn tất";
                btnNext.Enabled = true;
                btnCancel.Visible = false;
                btnNext.Location = new Point(418, 13);
            }
        }

        private void OnNextClicked(object sender, EventArgs e)
        {
            if (pageIndex == 0)
            {
                ShowPage(1);
            }
            else if (pageIndex == 1)
            {
                ShowPage(2);
                Thread t = new Thread(RunInstallation);
                t.IsBackground = true;
                t.Start();
            }
            else if (pageIndex == 3)
            {
                if (chkLaunchNow != null && chkLaunchNow.Checked && File.Exists(installedExePath))
                {
                    try
                    {
                        ProcessStartInfo psi = new ProcessStartInfo(installedExePath);
                        psi.WorkingDirectory = Path.GetDirectoryName(installedExePath);
                        Process.Start(psi);
                    }
                    catch { }
                }
                this.Close();
            }
        }

        private void LogStep(int pct, string status, string logLine)
        {
            if (this.InvokeRequired)
            {
                this.Invoke(new Action(() => LogStep(pct, status, logLine)));
                return;
            }
            progressBar.Value = Math.Min(100, Math.Max(0, pct));
            lblProgressStatus.Text = status;
            if (!string.IsNullOrEmpty(logLine))
            {
                lstLog.Items.Add(logLine);
                lstLog.TopIndex = lstLog.Items.Count - 1;
            }
        }

        private void RunInstallation()
        {
            try
            {
                string targetDir = txtInstallDir.Text.Trim();
                bool makeDesktop = chkDesktop.Checked;
                bool makeStart = chkStartMenu.Checked;

                LogStep(10, "Đang tạo thư mục cài đặt...", "Tạo thư mục: " + targetDir);
                Directory.CreateDirectory(targetDir);
                Thread.Sleep(250);

                // Stop any running instance of VN Launcher.exe in targetDir so file can be updated
                foreach (Process p in Process.GetProcessesByName("VN Launcher"))
                {
                    try { p.Kill(); p.WaitForExit(1500); } catch { }
                }

                string targetExe = Path.Combine(targetDir, "VN Launcher.exe");
                LogStep(35, "Đang trích xuất ứng dụng VN Launcher.exe...", "Giải nén: VN Launcher.exe -> " + targetExe);

                using (Stream resStream = Assembly.GetExecutingAssembly().GetManifestResourceStream("VNLauncher.exe"))
                {
                    if (resStream == null)
                        throw new Exception("Không tìm thấy gói tài nguyên VNLauncher.exe bên trong bộ cài.");
                    using (FileStream fs = new FileStream(targetExe, FileMode.Create, FileAccess.Write))
                    {
                        resStream.CopyTo(fs);
                    }
                }
                Thread.Sleep(300);

                LogStep(60, "Đang cấu hình thư mục dữ liệu trong AppData...", "Lưu cấu hình: %APPDATA%\\VNLauncher\\vn_launcher_config.json");
                string cfgDir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "VNLauncher");
                Directory.CreateDirectory(cfgDir);
                string cfgFile = Path.Combine(cfgDir, "vn_launcher_config.json");
                string defaultAppDataDir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), ".vn_launcher");
                string customDirEscaped = (string.Equals(Path.GetFullPath(targetDir), Path.GetFullPath(defaultAppDataDir), StringComparison.OrdinalIgnoreCase))
                    ? ""
                    : targetDir.Replace("\\", "\\\\");

                if (!File.Exists(cfgFile))
                {
                    string initJson = "{\n  \"config_version\": 2,\n  \"username\": \"\",\n  \"portable_mode\": false,\n  \"custom_data_dir\": \"" + customDirEscaped + "\",\n  \"custom_java_path\": \"\",\n  \"selected_instance_id\": \"\",\n  \"instances\": [],\n  \"saved_servers\": [],\n  \"minimize_on_play\": false,\n  \"show_snapshots\": false,\n  \"ms_client_id\": \"\"\n}";
                    File.WriteAllText(cfgFile, initJson);
                }
                Thread.Sleep(250);

                if (makeStart)
                {
                    string startLnk = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), @"Microsoft\Windows\Start Menu\Programs\VN Launcher.lnk");
                    LogStep(75, "Đang tạo lối tắt trong Start Menu...", "Tạo lối tắt Start Menu: " + startLnk);
                    CreateShortcut(startLnk, targetExe, targetDir);
                }

                if (makeDesktop)
                {
                    string deskLnk = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory), "VN Launcher.lnk");
                    LogStep(85, "Đang tạo lối tắt ngoài Desktop...", "Tạo lối tắt Desktop: " + deskLnk);
                    CreateShortcut(deskLnk, targetExe, targetDir);
                }
                Thread.Sleep(250);

                LogStep(95, "Đang đăng ký ứng dụng với Windows...", "Đăng ký Registry: HKCU\\...\\Uninstall\\VNLauncher");
                using (RegistryKey k = Registry.CurrentUser.CreateSubKey(@"Software\Microsoft\Windows\CurrentVersion\Uninstall\VNLauncher"))
                {
                    if (k != null)
                    {
                        k.SetValue("DisplayName", "VN Launcher");
                        k.SetValue("DisplayIcon", targetExe + ",0");
                        k.SetValue("DisplayVersion", "2.0.0");
                        k.SetValue("Publisher", "Nguyễn Trọng Việt Anh (Vanh)");
                        k.SetValue("InstallLocation", targetDir);
                        k.SetValue("UninstallString", "\"" + targetExe + "\" --uninstall");
                        k.SetValue("NoModify", 1, RegistryValueKind.DWord);
                        k.SetValue("NoRepair", 1, RegistryValueKind.DWord);
                    }
                }
                foreach (string ap in new[] { "VN Launcher.exe", "vn-launcher.exe" })
                {
                    using (RegistryKey ak = Registry.CurrentUser.CreateSubKey(@"Software\Microsoft\Windows\CurrentVersion\App Paths\" + ap))
                    {
                        if (ak != null)
                        {
                            ak.SetValue("", targetExe);
                            ak.SetValue("Path", targetDir);
                        }
                    }
                }

                installedExePath = targetExe;
                LogStep(100, "Đã cài đặt hoàn tất!", "✓ Hoàn tất cài đặt VN Launcher 2.0!");
                Thread.Sleep(350);

                this.Invoke(new Action(() => ShowPage(3)));
            }
            catch (Exception ex)
            {
                this.Invoke(new Action(() =>
                {
                    MessageBox.Show(this, "Lỗi cài đặt: " + ex.Message, "VN Launcher Setup", MessageBoxButtons.OK, MessageBoxIcon.Error);
                    ShowPage(1);
                }));
            }
        }

        private static void CreateShortcut(string lnkPath, string targetExe, string workDir)
        {
            Type t = Type.GetTypeFromProgID("WScript.Shell");
            if (t == null) return;
            dynamic shell = Activator.CreateInstance(t);
            dynamic sc = shell.CreateShortcut(lnkPath);
            sc.TargetPath = targetExe;
            sc.WorkingDirectory = workDir;
            sc.IconLocation = targetExe + ",0";
            sc.Description = "VN Launcher - Minecraft: Java Edition Launcher";
            sc.Save();
        }

        private static void SilentUpdate(string targetDir)
        {
            try
            {
                if (string.IsNullOrWhiteSpace(targetDir))
                {
                    targetDir = Path.Combine(
                        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
                        ".vn_launcher"
                    );
                }
                Directory.CreateDirectory(targetDir);
                string targetExe = Path.Combine(targetDir, "VN Launcher.exe");

                for (int i = 0; i < 15; i++)
                {
                    var procs = Process.GetProcessesByName("VN Launcher");
                    var procs2 = Process.GetProcessesByName("vn-launcher");
                    if (procs.Length == 0 && procs2.Length == 0) break;
                    if (i >= 5)
                    {
                        foreach (var p in procs) { try { p.Kill(); } catch { } }
                        foreach (var p in procs2) { try { p.Kill(); } catch { } }
                    }
                    Thread.Sleep(250);
                }

                using (Stream res = Assembly.GetExecutingAssembly().GetManifestResourceStream("VNLauncher.exe"))
                {
                    if (res == null) throw new Exception("Không tìm thấy gói ứng dụng nhúng.");
                    using (FileStream fs = File.Create(targetExe))
                    {
                        res.CopyTo(fs);
                    }
                }

                try
                {
                    using (RegistryKey k = Registry.CurrentUser.CreateSubKey(@"Software\Microsoft\Windows\CurrentVersion\Uninstall\VNLauncher"))
                    {
                        if (k != null) k.SetValue("DisplayVersion", "2.1.2");
                    }
                }
                catch { }

                Process.Start(new ProcessStartInfo(targetExe) { WorkingDirectory = targetDir, UseShellExecute = true });
            }
            catch (Exception ex)
            {
                MessageBox.Show("Lỗi cập nhật VN Launcher: " + ex.Message, "VN Launcher Update", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }

        [STAThread]
        public static void Main(string[] args)
        {
            if (args != null && args.Length > 0 && args[0].Equals("/silent", StringComparison.OrdinalIgnoreCase))
            {
                string dir = args.Length > 1 ? args[1] : "";
                SilentUpdate(dir);
                return;
            }
            try { SetProcessDPIAware(); } catch { }
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            Application.Run(new SetupWizard());
        }
    }
}
