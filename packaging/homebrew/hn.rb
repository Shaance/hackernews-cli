class Hn < Formula
  desc "Read Hacker News stories and comment threads in your terminal"
  homepage "https://github.com/Shaance/hn"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/Shaance/hn/releases/download/tui-v@VERSION@/hn-@VERSION@-aarch64-apple-darwin.tar.gz"
      sha256 "@SHA256_aarch64-apple-darwin@"
    end
    on_intel do
      url "https://github.com/Shaance/hn/releases/download/tui-v@VERSION@/hn-@VERSION@-x86_64-apple-darwin.tar.gz"
      sha256 "@SHA256_x86_64-apple-darwin@"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/Shaance/hn/releases/download/tui-v@VERSION@/hn-@VERSION@-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA256_aarch64-unknown-linux-gnu@"
    end
    on_intel do
      url "https://github.com/Shaance/hn/releases/download/tui-v@VERSION@/hn-@VERSION@-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA256_x86_64-unknown-linux-gnu@"
    end

    depends_on "openssl@3"
  end

  def install
    bin.install "hn"
  end

  test do
    assert_equal "hn #{version}\n", shell_output("#{bin}/hn --version")
  end
end
