@Component export struct Box<T> { @Require @Prop value!: T; build() {} } @Component export struct Card { @Prop title: string = ""; build() { Text(this.title) } }
