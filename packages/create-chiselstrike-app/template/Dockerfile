FROM frolvlad/alpine-glibc:alpine-3.13

COPY . /opt/app

WORKDIR /opt/app

RUN apk add npm

RUN npm i

EXPOSE 8080/tcp

# replace this with your own connection string if deploying into postgres!
ARG DBURI="sqlite://.chiseld.db?mode=rwc"

CMD npm run dev -- -- --db-uri ${DBURI} --api-listen-addr 0.0.0.0:8080
