### ToDo App

A Sample todo api service for creating/updating todos


#### Components:

##### API
The api is a simple rust-warp app written in a single file in src/main.rs
This uses a cache & SQL db to store the todo's

###### endpoints:
- GET /todos  => List all todo's
- POST /todo => Create a todo with a json request body
- POST /complete/:id => Complete/update an existing todo
- DEL /todo/:id => delete a todo object
- GET /todo/:id => get a single todo

##### Database:
The database being used in the example is `postgres` via sqlx
This is an external image from the docker hub

##### Cache:
Redis is used for caching the GET todo/:id endpoint

the docker version uses a single instance
whereas kubernetes allows for non-clustered (single-master) replicas
(I've chosen to forgo cluster support as it requires a minimum of 6 nodes which is overkill for our setup)

you can either run a helm based service for redis using [values.yaml](./kubernetes/redis/helm/values.yaml) which support auto failover using redis sentinel or manage it manually via [vanilla kubernetes configs](./kubernetes/redis/vanilla/) 



### Installation/Running

this can be run via either docker-compose or via kubernetes

##### Running via docker-compose:

there's a [`docker-compose.yml`](./docker-compose.yml) that you can use to start the swarm consisting of redis, postgres & api server

```bash
docker-compose up -d

```

##### Running via kubernetes/helm

1. Create a new namespace for your app
```bash
kubectl create ns todo
```
2. Add storage related configuration for postgres (& redis if you are not using helm)
```
kubectl -n todo apply -f ./kubernetes/storage.yaml
```
3. Add redis clusters
```
kubectl -n todo apply -f ./kubernetes/redis/vanilla/redis-config.yaml
kubectl -n todo apply -f ./kubernetes/redis/vanilla/redis-statefulset.yaml
kubectl -n todo apply -f ./kubernetes/redis/vanilla/redis-service.yaml
```
OR
```
helm repo add bitnami https://charts.bitnami.com/bitnami
helm install redis-sentinel bitnami/redis --values /kubernetes/redis/helm/values.yaml
```

4. Add postgres configuration & deployment
```
kubectl -n todo apply -f ./kubernetes/postgres/postgres-config.yaml
kubectl -n todo apply -f ./kubernetes/postgres/postgres-service.yaml
kubectl -n todo apply -f ./kubernetes/postgres/postgres-statefulset.yaml
```

5. Deploy your app
```
kubectl -n todo apply -f ./kubernetes/server/server-config.yaml
kubectl -n todo apply -f ./kubernetes/server/server.yaml
```



#### (Possible TODO's)
- Implement an agnostic failover mechanism for redis by updating the dns values using the client-reconfig option or listening for sentinel notifications
- implement clustering/replication for postgres
