"""
Kubernetes client wrapper
"""

from kubernetes import client, config
import os


def get_k8s_client():
    """Get configured Kubernetes client"""
    try:
        if os.getenv("KUBERNETES_SERVICE_HOST"):
            config.load_incluster_config()
        else:
            config.load_kube_config()
    except Exception:
        config.load_kube_config()

    return {
        "apps": client.AppsV1Api(),
        "core": client.CoreV1Api(),
        "custom": client.CustomObjectsApi(),
    }
