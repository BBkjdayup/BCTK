using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.Text;
using System.Windows.Forms;

[assembly: System.Reflection.AssemblyTitle("TK试题题库授权工具")]
[assembly: System.Reflection.AssemblyDescription("TK试题题库离线许可证签发工具")]
[assembly: System.Reflection.AssemblyCompany("Zhitiku")]
[assembly: System.Reflection.AssemblyProduct("TK试题题库授权工具")]
[assembly: System.Reflection.AssemblyVersion("1.0.0.0")]

namespace TkQuestionBankLicenseIssuer
{
    internal static class Program
    {
        [STAThread]
        private static void Main()
        {
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            Application.Run(new IssuerForm());
        }
    }

    internal sealed class BackendResult
    {
        public int ExitCode;
        public string StandardOutput = string.Empty;
        public string StandardError = string.Empty;
    }

    internal sealed class IssuerForm : Form
    {
        private readonly TextBox _requestPath = CreatePathBox();
        private readonly TextBox _keyPath = CreatePathBox();
        private readonly TextBox _customerName = new TextBox();
        private readonly DateTimePicker _expiry = new DateTimePicker();
        private readonly TextBox _outputPath = CreatePathBox();
        private readonly Label _deviceValue = CreateValueLabel();
        private readonly Label _versionValue = CreateValueLabel();
        private readonly Label _generatedValue = CreateValueLabel();
        private readonly Label _keyStatus = CreateValueLabel();
        private readonly Label _status = new Label();
        private readonly Button _issueButton = new Button();
        private readonly Button _openFolderButton = new Button();

        private string _deviceId = string.Empty;
        private bool _requestVerified;
        private bool _keyVerified;

        public IssuerForm()
        {
            Text = "TK试题题库授权工具";
            StartPosition = FormStartPosition.CenterScreen;
            ClientSize = new Size(820, 650);
            MinimumSize = new Size(836, 689);
            MaximizeBox = false;
            Font = new Font("Microsoft YaHei UI", 9F, FontStyle.Regular, GraphicsUnit.Point);
            BackColor = Color.FromArgb(245, 248, 252);
            AutoScaleMode = AutoScaleMode.Dpi;

            BuildInterface();
        }

        private static TextBox CreatePathBox()
        {
            return new TextBox
            {
                ReadOnly = true,
                BackColor = Color.White,
                BorderStyle = BorderStyle.FixedSingle
            };
        }

        private static Label CreateValueLabel()
        {
            return new Label
            {
                AutoEllipsis = true,
                ForeColor = Color.FromArgb(30, 41, 59),
                Text = "尚未选择",
                TextAlign = ContentAlignment.MiddleLeft
            };
        }

        private void BuildInterface()
        {
            Label title = AddLabel("离线许可证签发", 28, 20, 300, 34, 17F, FontStyle.Bold);
            title.ForeColor = Color.FromArgb(15, 23, 42);
            Label subtitle = AddLabel(
                "只在授权负责人的电脑上使用；不要把本工具、签发私钥或整个文件夹交给客户。",
                30, 55, 750, 24, 9F, FontStyle.Regular);
            subtitle.ForeColor = Color.FromArgb(100, 116, 139);

            AddLabel("客户授权申请（.tkreq）", 30, 94, 230, 24, 9F, FontStyle.Bold);
            Place(_requestPath, 30, 120, 650, 29);
            Button requestButton = CreateButton("选择申请文件…", 694, 118, 96, 32);
            requestButton.Click += delegate { SelectRequest(); };

            AddLabel("签发私钥", 30, 164, 230, 24, 9F, FontStyle.Bold);
            Place(_keyPath, 30, 190, 650, 29);
            Button keyButton = CreateButton("选择私钥…", 694, 188, 96, 32);
            keyButton.Click += delegate { SelectPrivateKey(); };

            GroupBox requestInfo = new GroupBox
            {
                Text = "已验证的申请信息",
                Location = new Point(30, 236),
                Size = new Size(760, 108),
                BackColor = Color.White,
                ForeColor = Color.FromArgb(51, 65, 85)
            };
            Controls.Add(requestInfo);
            AddGroupLabel(requestInfo, "设备编号", 18, 27, 82);
            PlaceIn(requestInfo, _deviceValue, 102, 24, 635, 24);
            AddGroupLabel(requestInfo, "软件版本", 18, 57, 82);
            PlaceIn(requestInfo, _versionValue, 102, 54, 160, 24);
            AddGroupLabel(requestInfo, "申请时间", 300, 57, 82);
            PlaceIn(requestInfo, _generatedValue, 384, 54, 353, 24);

            AddLabel("客户名称", 30, 364, 180, 24, 9F, FontStyle.Bold);
            Place(_customerName, 30, 390, 360, 29);
            _customerName.MaxLength = 100;
            _customerName.PlaceholderTextCompat("例如：某某学校 / 张老师");

            AddLabel("许可证到期时间", 430, 364, 180, 24, 9F, FontStyle.Bold);
            _expiry.Format = DateTimePickerFormat.Custom;
            _expiry.CustomFormat = "yyyy-MM-dd HH:mm:ss";
            _expiry.Value = DateTime.Today.AddYears(1).AddDays(1).AddSeconds(-1);
            Place(_expiry, 430, 390, 360, 29);

            AddLabel("许可证保存位置（.tklic）", 30, 438, 250, 24, 9F, FontStyle.Bold);
            Place(_outputPath, 30, 464, 650, 29);
            Button outputButton = CreateButton("选择保存位置…", 694, 462, 96, 32);
            outputButton.Click += delegate { SelectOutput(); };

            _issueButton.Text = "验证并签发许可证";
            _issueButton.Location = new Point(30, 520);
            _issueButton.Size = new Size(180, 40);
            _issueButton.BackColor = Color.FromArgb(37, 99, 235);
            _issueButton.ForeColor = Color.White;
            _issueButton.FlatStyle = FlatStyle.Flat;
            _issueButton.FlatAppearance.BorderSize = 0;
            _issueButton.Click += delegate { IssueLicense(); };
            Controls.Add(_issueButton);

            _openFolderButton.Text = "打开许可证所在文件夹";
            _openFolderButton.Location = new Point(224, 520);
            _openFolderButton.Size = new Size(176, 40);
            _openFolderButton.Enabled = false;
            _openFolderButton.Click += delegate { OpenOutputFolder(); };
            Controls.Add(_openFolderButton);

            _keyStatus.Location = new Point(420, 520);
            _keyStatus.Size = new Size(370, 40);
            _keyStatus.ForeColor = Color.FromArgb(100, 116, 139);
            Controls.Add(_keyStatus);

            Panel statusPanel = new Panel
            {
                Location = new Point(0, 582),
                Size = new Size(820, 68),
                BackColor = Color.White
            };
            Controls.Add(statusPanel);
            _status.Location = new Point(30, 11);
            _status.Size = new Size(760, 46);
            _status.Text = "请选择客户授权申请和签发私钥。私钥内容不会被保存到本工具中。";
            _status.ForeColor = Color.FromArgb(71, 85, 105);
            _status.TextAlign = ContentAlignment.MiddleLeft;
            statusPanel.Controls.Add(_status);
        }

        private Label AddLabel(string text, int x, int y, int width, int height, float size, FontStyle style)
        {
            Label label = new Label
            {
                Text = text,
                Location = new Point(x, y),
                Size = new Size(width, height),
                Font = new Font("Microsoft YaHei UI", size, style, GraphicsUnit.Point),
                ForeColor = Color.FromArgb(51, 65, 85),
                TextAlign = ContentAlignment.MiddleLeft
            };
            Controls.Add(label);
            return label;
        }

        private static void AddGroupLabel(Control parent, string text, int x, int y, int width)
        {
            parent.Controls.Add(new Label
            {
                Text = text,
                Location = new Point(x, y),
                Size = new Size(width, 24),
                ForeColor = Color.FromArgb(100, 116, 139),
                TextAlign = ContentAlignment.MiddleLeft
            });
        }

        private Button CreateButton(string text, int x, int y, int width, int height)
        {
            Button button = new Button
            {
                Text = text,
                Location = new Point(x, y),
                Size = new Size(width, height),
                BackColor = Color.White,
                UseVisualStyleBackColor = false
            };
            Controls.Add(button);
            return button;
        }

        private void Place(Control control, int x, int y, int width, int height)
        {
            control.Location = new Point(x, y);
            control.Size = new Size(width, height);
            Controls.Add(control);
        }

        private static void PlaceIn(Control parent, Control control, int x, int y, int width, int height)
        {
            control.Location = new Point(x, y);
            control.Size = new Size(width, height);
            parent.Controls.Add(control);
        }

        private void SelectRequest()
        {
            using (OpenFileDialog dialog = new OpenFileDialog())
            {
                dialog.Title = "选择客户发来的授权申请";
                dialog.Filter = "TK授权申请 (*.tkreq)|*.tkreq";
                dialog.CheckFileExists = true;
                dialog.Multiselect = false;
                if (dialog.ShowDialog(this) != DialogResult.OK) return;

                _requestPath.Text = dialog.FileName;
                VerifyRequest(dialog.FileName);
            }
        }

        private void VerifyRequest(string path)
        {
            SetBusy(true, "正在验证授权申请签名…");
            try
            {
                BackendResult result = RunBackend("inspect-request", path);
                if (result.ExitCode != 0)
                {
                    ResetRequestInfo();
                    ShowError("授权申请验证失败", BackendMessage(result));
                    return;
                }
                Dictionary<string, string> fields = ParseFields(result.StandardOutput);
                string deviceId;
                string appVersion;
                string generatedAt;
                if (!fields.TryGetValue("DEVICE_ID", out deviceId)
                    || !fields.TryGetValue("APP_VERSION", out appVersion)
                    || !fields.TryGetValue("GENERATED_AT_MS", out generatedAt))
                {
                    ResetRequestInfo();
                    ShowError("授权申请验证失败", "签名后端返回的信息不完整。请确认工具文件没有被替换。");
                    return;
                }
                _deviceId = deviceId;
                _deviceValue.Text = deviceId;
                _versionValue.Text = "v" + appVersion;
                _generatedValue.Text = FormatUnixMilliseconds(generatedAt);
                _requestVerified = true;
                SetStatus("授权申请签名有效，设备信息已读取。", false);
            }
            catch (Exception error)
            {
                ResetRequestInfo();
                ShowError("授权申请验证失败", error.Message);
            }
            finally
            {
                SetBusy(false, null);
            }
        }

        private void SelectPrivateKey()
        {
            using (OpenFileDialog dialog = new OpenFileDialog())
            {
                dialog.Title = "选择离线保存的签发私钥";
                dialog.Filter = "TK签发私钥 (*.key)|*.key|所有文件 (*.*)|*.*";
                dialog.CheckFileExists = true;
                dialog.Multiselect = false;
                if (dialog.ShowDialog(this) != DialogResult.OK) return;

                _keyPath.Text = dialog.FileName;
                VerifyPrivateKey(dialog.FileName);
            }
        }

        private void VerifyPrivateKey(string path)
        {
            SetBusy(true, "正在确认签发私钥与客户软件匹配…");
            try
            {
                BackendResult result = RunBackend("inspect-key", path);
                if (result.ExitCode != 0)
                {
                    _keyVerified = false;
                    _keyStatus.Text = "私钥未通过校验";
                    _keyStatus.ForeColor = Color.FromArgb(220, 38, 38);
                    ShowError("签发私钥不可用", BackendMessage(result));
                    return;
                }
                Dictionary<string, string> fields = ParseFields(result.StandardOutput);
                string keyId;
                fields.TryGetValue("KEY_ID", out keyId);
                _keyVerified = true;
                _keyStatus.Text = "签发密钥已验证 · " + (keyId ?? "当前生产密钥");
                _keyStatus.ForeColor = Color.FromArgb(22, 163, 74);
                SetStatus("签发私钥与当前客户软件匹配。", false);
            }
            catch (Exception error)
            {
                _keyVerified = false;
                _keyStatus.Text = "私钥未通过校验";
                _keyStatus.ForeColor = Color.FromArgb(220, 38, 38);
                ShowError("签发私钥不可用", error.Message);
            }
            finally
            {
                SetBusy(false, null);
            }
        }

        private void SelectOutput()
        {
            using (SaveFileDialog dialog = new SaveFileDialog())
            {
                dialog.Title = "保存要发给客户的许可证";
                dialog.Filter = "TK桌面许可证 (*.tklic)|*.tklic";
                dialog.AddExtension = true;
                dialog.DefaultExt = "tklic";
                dialog.OverwritePrompt = false;
                string safeCustomer = SafeFileName(_customerName.Text);
                dialog.FileName = string.IsNullOrEmpty(safeCustomer)
                    ? "TK试题题库桌面许可证.tklic"
                    : safeCustomer + "-TK试题题库桌面许可证.tklic";
                if (dialog.ShowDialog(this) != DialogResult.OK) return;
                if (File.Exists(dialog.FileName))
                {
                    ShowError("不能覆盖已有文件", "请在保存窗口中换一个新文件名，避免误覆盖已经签发的许可证。");
                    return;
                }
                _outputPath.Text = dialog.FileName;
                _openFolderButton.Enabled = false;
            }
        }

        private void IssueLicense()
        {
            string customer = _customerName.Text.Trim();
            if (!_requestVerified || !File.Exists(_requestPath.Text))
            {
                ShowError("尚未验证授权申请", "请先选择并成功验证客户发来的 .tkreq 文件。");
                return;
            }
            if (!_keyVerified || !File.Exists(_keyPath.Text))
            {
                ShowError("尚未验证签发私钥", "请先选择与当前客户软件匹配的签发私钥。");
                return;
            }
            if (customer.Length == 0 || customer.Length > 100)
            {
                ShowError("客户名称无效", "客户名称不能为空且不能超过 100 个字符。");
                _customerName.Focus();
                return;
            }
            if (_expiry.Value <= DateTime.Now)
            {
                ShowError("到期时间无效", "许可证到期时间必须晚于当前时间。");
                return;
            }
            if (string.IsNullOrWhiteSpace(_outputPath.Text))
            {
                ShowError("尚未选择保存位置", "请选择一个新的 .tklic 文件保存位置。");
                return;
            }
            if (File.Exists(_outputPath.Text))
            {
                ShowError("不能覆盖已有文件", "目标许可证文件已经存在，请重新选择文件名。");
                return;
            }

            string confirmation = "请核对本次签发信息：\n\n"
                + "客户：" + customer + "\n"
                + "设备：" + ShortDeviceId(_deviceId) + "\n"
                + "到期：" + _expiry.Value.ToString("yyyy-MM-dd HH:mm:ss") + "\n\n"
                + "生成后请只把 .tklic 发给该客户，不要发送私钥或授权工具。";
            if (MessageBox.Show(this, confirmation, "确认签发许可证", MessageBoxButtons.OKCancel,
                    MessageBoxIcon.Warning, MessageBoxDefaultButton.Button2) != DialogResult.OK)
            {
                return;
            }

            SetBusy(true, "正在签名并生成许可证…");
            try
            {
                string expiresAt = ToUnixMilliseconds(_expiry.Value).ToString(CultureInfo.InvariantCulture);
                BackendResult result = RunBackend(
                    "issue", _keyPath.Text, _requestPath.Text, _outputPath.Text, customer, expiresAt);
                if (result.ExitCode != 0 || !File.Exists(_outputPath.Text))
                {
                    ShowError("许可证签发失败", BackendMessage(result));
                    return;
                }
                _openFolderButton.Enabled = true;
                SetStatus("许可证签发成功：" + _outputPath.Text, false);
                MessageBox.Show(this,
                    "许可证已经生成。\n\n请把 .tklic 文件发给对应客户；不要发送签发私钥或本授权工具。",
                    "签发成功", MessageBoxButtons.OK, MessageBoxIcon.Information);
            }
            catch (Exception error)
            {
                ShowError("许可证签发失败", error.Message);
            }
            finally
            {
                SetBusy(false, null);
            }
        }

        private BackendResult RunBackend(params string[] arguments)
        {
            string backendPath = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "license_admin.exe");
            if (!File.Exists(backendPath))
            {
                throw new FileNotFoundException("签名后端 license_admin.exe 不在授权工具同一目录中。", backendPath);
            }

            ProcessStartInfo start = new ProcessStartInfo
            {
                FileName = backendPath,
                Arguments = JoinArguments(arguments),
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                StandardOutputEncoding = Encoding.UTF8,
                StandardErrorEncoding = Encoding.UTF8,
                WorkingDirectory = AppDomain.CurrentDomain.BaseDirectory
            };
            using (Process process = Process.Start(start))
            {
                if (process == null) throw new InvalidOperationException("无法启动许可证签名后端。");
                if (!process.WaitForExit(30000))
                {
                    process.Kill();
                    throw new TimeoutException("许可证签名后端在 30 秒内没有完成。");
                }
                return new BackendResult
                {
                    ExitCode = process.ExitCode,
                    StandardOutput = process.StandardOutput.ReadToEnd(),
                    StandardError = process.StandardError.ReadToEnd()
                };
            }
        }

        private void ResetRequestInfo()
        {
            _requestVerified = false;
            _deviceId = string.Empty;
            _deviceValue.Text = "验证失败";
            _versionValue.Text = "—";
            _generatedValue.Text = "—";
        }

        private void SetBusy(bool busy, string message)
        {
            UseWaitCursor = busy;
            _issueButton.Enabled = !busy;
            if (!string.IsNullOrEmpty(message)) SetStatus(message, false);
            Application.DoEvents();
        }

        private void SetStatus(string message, bool error)
        {
            _status.Text = message;
            _status.ForeColor = error ? Color.FromArgb(185, 28, 28) : Color.FromArgb(71, 85, 105);
        }

        private void ShowError(string title, string message)
        {
            SetStatus(title + "：" + message, true);
            MessageBox.Show(this, message, title, MessageBoxButtons.OK, MessageBoxIcon.Error);
        }

        private void OpenOutputFolder()
        {
            if (!File.Exists(_outputPath.Text)) return;
            Process.Start("explorer.exe", "/select," + QuoteArgument(_outputPath.Text));
        }

        private static Dictionary<string, string> ParseFields(string output)
        {
            Dictionary<string, string> fields = new Dictionary<string, string>(StringComparer.Ordinal);
            string[] lines = output.Replace("\r", string.Empty).Split('\n');
            foreach (string line in lines)
            {
                int separator = line.IndexOf('=');
                if (separator <= 0) continue;
                fields[line.Substring(0, separator)] = line.Substring(separator + 1);
            }
            return fields;
        }

        private static string BackendMessage(BackendResult result)
        {
            string message = string.IsNullOrWhiteSpace(result.StandardError)
                ? result.StandardOutput
                : result.StandardError;
            message = message.Trim();
            return string.IsNullOrEmpty(message) ? "签名后端没有返回详细错误。" : message;
        }

        private static long ToUnixMilliseconds(DateTime localTime)
        {
            DateTime utc = localTime.ToUniversalTime();
            DateTime epoch = new DateTime(1970, 1, 1, 0, 0, 0, DateTimeKind.Utc);
            return checked((long)(utc - epoch).TotalMilliseconds);
        }

        private static string FormatUnixMilliseconds(string value)
        {
            long milliseconds;
            if (!long.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out milliseconds))
                return "时间格式无效";
            DateTime epoch = new DateTime(1970, 1, 1, 0, 0, 0, DateTimeKind.Utc);
            try
            {
                return epoch.AddMilliseconds(milliseconds).ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss");
            }
            catch
            {
                return "时间超出支持范围";
            }
        }

        private static string SafeFileName(string value)
        {
            string result = value.Trim();
            foreach (char invalid in Path.GetInvalidFileNameChars()) result = result.Replace(invalid, '_');
            return result.Length > 40 ? result.Substring(0, 40) : result;
        }

        private static string ShortDeviceId(string value)
        {
            return value.Length <= 20 ? value : value.Substring(0, 12) + "…" + value.Substring(value.Length - 8);
        }

        private static string JoinArguments(IEnumerable<string> arguments)
        {
            StringBuilder builder = new StringBuilder();
            foreach (string argument in arguments)
            {
                if (builder.Length > 0) builder.Append(' ');
                builder.Append(QuoteArgument(argument));
            }
            return builder.ToString();
        }

        private static string QuoteArgument(string argument)
        {
            if (argument.Length > 0 && argument.IndexOfAny(new[] { ' ', '\t', '\n', '\v', '"' }) < 0)
                return argument;
            StringBuilder quoted = new StringBuilder();
            quoted.Append('"');
            int backslashes = 0;
            foreach (char character in argument)
            {
                if (character == '\\')
                {
                    backslashes++;
                    continue;
                }
                if (character == '"')
                {
                    quoted.Append('\\', backslashes * 2 + 1);
                    quoted.Append('"');
                    backslashes = 0;
                    continue;
                }
                quoted.Append('\\', backslashes);
                backslashes = 0;
                quoted.Append(character);
            }
            quoted.Append('\\', backslashes * 2);
            quoted.Append('"');
            return quoted.ToString();
        }
    }

    internal static class TextBoxCompatibility
    {
        public static void PlaceholderTextCompat(this TextBox textBox, string placeholder)
        {
            textBox.Tag = placeholder;
            textBox.ForeColor = Color.FromArgb(51, 65, 85);
        }
    }
}
