# Homebrew formula for guise.
#
# To publish a tap:
#   1. Create a repo named `homebrew-tap` under your GitHub account.
#   2. Copy this file to `Formula/guise.rb` in that repo.
#   3. After each release, update `version`, `url`, and `sha256`
#      (the sha is published as guise-macos.tar.gz.sha256 on the release).
#
# Users then install with:
#   brew install siddhjagani/tap/guise
class Guise < Formula
  desc "Run all your Claude Desktop accounts at once, each in its own window"
  homepage "https://github.com/siddhjagani/guise"
  version "0.1.0"
  url "https://github.com/siddhjagani/guise/releases/download/v0.1.0/guise-macos.tar.gz"
  sha256 "REPLACE_WITH_SHA256_FROM_RELEASE"
  license "MIT"
  depends_on :macos

  def install
    bin.install "guise"
  end

  test do
    assert_match "guise", shell_output("#{bin}/guise --version")
  end
end
