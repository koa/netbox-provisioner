# NetBox configuration of the local lab instance (local/netbox/run-netbox.sh), not for
# production: fixed secrets, debug, everything on localhost.
import os

_data = os.environ["LAB_NETBOX_DATA"]

ALLOWED_HOSTS = ["*"]
DATABASE = {
    "NAME": "netbox",
    "USER": os.environ.get("USER", "netbox"),
    # unix socket of the lab's own PostgreSQL
    "HOST": os.path.join(_data, "pg-socket"),
    "PORT": "",
}
REDIS = {
    "tasks": {"HOST": "127.0.0.1", "PORT": int(os.environ["LAB_REDIS_PORT"]), "DATABASE": 0, "SSL": False},
    "caching": {"HOST": "127.0.0.1", "PORT": int(os.environ["LAB_REDIS_PORT"]), "DATABASE": 1, "SSL": False},
}
SECRET_KEY = "lab-only-secret-key-not-for-production-0123456789abcdefghijklmnopqrstuvwxyz"
API_TOKEN_PEPPERS = {1: "lab-only-pepper-not-for-production-0123456789abcdefghijkl"}
MEDIA_ROOT = os.path.join(_data, "media")
STATIC_ROOT = os.path.join(_data, "static")
REPORTS_ROOT = os.path.join(_data, "reports")
SCRIPTS_ROOT = os.path.join(_data, "scripts")
DEBUG = True
TIME_ZONE = "Europe/Zurich"

# Login over the lab's Keycloak (local/keycloak, client "netbox"), as production does over
# Pocket-ID; the local admin/admin keeps working. A user the login creates gets the group
# „Anwohner“ (local/netbox/seed-residents.py).
REMOTE_AUTH_ENABLED = True
REMOTE_AUTH_BACKEND = "social_core.backends.open_id_connect.OpenIdConnectAuth"
REMOTE_AUTH_DEFAULT_GROUPS = ["Anwohner"]
SOCIAL_AUTH_OIDC_OIDC_ENDPOINT = "http://localhost:8180/realms/lab"
SOCIAL_AUTH_OIDC_KEY = "netbox"
SOCIAL_AUTH_OIDC_SECRET = "lab-only-netbox-client-secret"
