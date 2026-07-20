"""Opt-in proxy integration isolated behind stable runtime/router hooks."""


def install_hub_api(app) -> None:
    """Install proxy routes when the host exposes FastAPI router support."""
    if not hasattr(app, "include_router"):
        return
    from controller.proxy.hub_api import router
    app.include_router(router)


def install_node_api(app) -> None:
    """Install proxy node routes without importing them in RPC-only tests."""
    if not hasattr(app, "include_router"):
        return
    from controller.proxy.node_api import router
    app.include_router(router)
