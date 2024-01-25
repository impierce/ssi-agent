import './body.css'
import { JSX } from "preact/jsx-runtime"

type Props = {
    children: string | JSX.Element | JSX.Element[]
}

export function Body({ children }: Props) {
    return (
        <div class="body">
            {children}
        </div>
    )
}
