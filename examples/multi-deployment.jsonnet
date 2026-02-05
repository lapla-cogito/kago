local deployment(name, image, replicas=1, cpu="100m", memory="128Mi", maxSurge=1, maxUnavailable=0) = {
  kind: "Deployment",
  spec: {
    name: name,
    image: image,
    replicas: replicas,
    resources: {
      cpu: cpu,
      memory: memory,
    },
    rolling_update: {
      max_surge: maxSurge,
      max_unavailable: maxUnavailable,
    },
  },
};

local defaultReplicas = 2;
local memoryTier = {
  small: "128Mi",
  medium: "256Mi",
  large: "512Mi",
};

[
  deployment("web", "nginx:alpine", defaultReplicas, "100m", memoryTier.small),
  deployment("api", "httpd:alpine", defaultReplicas, "200m", memoryTier.medium),
  deployment("cache", "redis:alpine", 1, "150m", memoryTier.large),
]
