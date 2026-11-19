class Hn < Formula
  desc "Read Hacker News stories and comment threads in your terminal"
  homepage "https://github.com/Shaance/hn"
  license "MIT"

  # Replace each placeholder with the hash from the matching release asset's .sha256 file.
  on_macos do
    on_arm do
      url "https://github.com/Shaance/hn/releases/download/tui-v0.2.0/hn-0.2.0-aarch64-apple-darwin.tar.gz"
      sha256 "FILL_ME_MACOS_ARM64"
    end
    on_intel do
      url "https://github.com/Shaance/hn/releases/download/tui-v0.2.0/hn-0.2.0-x86_64-apple-darwin.tar.gz"
      sha256 "FILL_ME_MACOS_X86_64"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/Shaance/hn/releases/download/tui-v0.2.0/hn-0.2.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "FILL_ME_LINUX_ARM64"
    end
    on_intel do
      url "https://github.com/Shaance/hn/releases/download/tui-v0.2.0/hn-0.2.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "FILL_ME_LINUX_X86_64"
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
