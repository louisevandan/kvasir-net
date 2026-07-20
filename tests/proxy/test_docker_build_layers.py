from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]


class DockerBuildLayerTests(unittest.TestCase):
    def test_runtime_dockerfile_cannot_compile_cuda(self):
        source = (ROOT / "docker" / "Dockerfile.cuda").read_text(encoding="utf-8")
        self.assertIn("ARG LINKCPP_ARTIFACT_IMAGE", source)
        self.assertIn("FROM ${LINKCPP_ARTIFACT_IMAGE} AS artifacts", source)
        self.assertIn("COPY controller /app/controller", source)
        self.assertNotIn("run cmake", source.lower())
        self.assertNotIn("build-essential", source.lower())
        self.assertNotIn("-devel-", source)

    def test_artifact_build_roles_are_separate(self):
        toolchain = (ROOT / "docker" / "cuda" / "Dockerfile.toolchain").read_text(encoding="utf-8")
        llama = (ROOT / "docker" / "cuda" / "Dockerfile.llama").read_text(encoding="utf-8")
        proxy = (ROOT / "docker" / "cuda" / "Dockerfile.proxy").read_text(encoding="utf-8")
        self.assertIn("CUDA_DEVEL_IMAGE", toolchain)
        self.assertNotIn("COPY external", toolchain)
        self.assertIn("LINKCPP_BUILD_RING_ADAPTER=OFF", llama)
        self.assertIn("FROM ${LLAMA_BUILD_IMAGE} AS proxy-build", proxy)
        self.assertIn("LINKCPP_BUILD_RING_ADAPTER=ON", proxy)

    def test_runtime_context_excludes_llama_sources(self):
        ignore = (ROOT / "docker" / "Dockerfile.cuda.dockerignore").read_text(encoding="utf-8")
        proxy_ignore = (ROOT / "docker" / "cuda" / "Dockerfile.proxy.dockerignore").read_text(encoding="utf-8")
        self.assertIn("!controller/**", ignore)
        self.assertNotIn("!external", ignore)
        self.assertNotIn("!external", proxy_ignore)
        self.assertIn("!apps/**", proxy_ignore)

    def test_compose_selects_prebuilt_artifacts(self):
        rpc = (ROOT / "docker-compose.cuda.yml").read_text(encoding="utf-8")
        proxy = (ROOT / "docker-compose.proxy.yml").read_text(encoding="utf-8")
        self.assertIn("LINKCPP_RPC_ARTIFACT_IMAGE", rpc)
        self.assertIn("LINKCPP_PROXY_ARTIFACT_IMAGE", proxy)
        self.assertNotIn("CUDA_ARCHS", rpc + proxy)
        self.assertNotIn("LINKCPP_BUILD_JOBS", rpc + proxy)

    def test_artifact_scripts_reject_ambiguous_dirty_builds(self):
        powershell = (ROOT / "scripts" / "build-cuda-artifacts.ps1").read_text(encoding="utf-8")
        shell = (ROOT / "scripts" / "build-cuda-artifacts.sh").read_text(encoding="utf-8")
        self.assertNotIn("AllowDirty", powershell)
        self.assertNotIn("ALLOW_DIRTY", shell)
        self.assertIn("require a clean worktree", powershell)


if __name__ == "__main__":
    unittest.main()
