class Workdeck < Formula
  desc "Terminal-native sidecar for agentic coding"
  homepage "https://github.com/workdeck/workdeck"
  license "MIT"
  head "https://github.com/workdeck/workdeck.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/workdeck-cli")
  end

  test do
    system "#{bin}/workdeck", "--version"
  end
end
