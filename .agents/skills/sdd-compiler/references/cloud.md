# Cloud Deployment Options

## Platform Comparison

| Platform | Best For | Services | Pricing |
|----------|----------|----------|---------|
| AWS | Enterprise | Full suite | Per-second |
| GCP | Data/ML | BigQuery, K8s | Per-second |
| Azure | Microsoft | AD, SQL | Per-second |
| Railway | Simplicity | Simple deploy | Free tier |
| Render | Static/API | Easy deploy | Free tier |
| Fly.io | Edge | Global | Free tier |

## Common Architectures

### Simple API (Node.js)
```
[Client] → [Load Balancer] → [API (N instances)]
                              ↓
                         [Database]
```

### With Cache
```
[Client] → [API] → [Redis Cache]
              ↘          ↙
              [Database]
```

### Full Stack
```
[Client] → [Cloudflare] → [API]
                           ↓
                    [Database]
                    [Cache]
                    [Queue]
```

## Container Options

### Docker
```dockerfile
FROM node:20-alpine
WORKDIR /app
COPY package*.json ./
RUN npm ci
COPY . .
EXPOSE 3000
CMD ["node", "index.js"]
```

### Docker Compose (dev)
```yaml
services:
  api:
    build: .
    ports:
      - "3000:3000"
    environment:
      - DATABASE_URL=postgres://user:pass@db:5432/app
    depends_on:
      - db
      - redis

  db:
    image: postgres:15
    environment:
      POSTGRES_USER: user
      POSTGRES_PASSWORD: pass
      POSTGRES_DB: app

  redis:
    image: redis:7
```

## Serverless

### AWS Lambda + API Gateway
- Pay per request
- Cold starts
- Max 29s timeout

### Vercel/Netlify Functions
- Easy for frontend devs
- Limited execution time